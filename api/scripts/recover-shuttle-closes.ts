#!/usr/bin/env node

import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";

import { EPHEMERAL_SPL_TOKEN_PROGRAM_ID } from "@magicblock-labs/ephemeral-rollups-sdk";
import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  VersionedTransaction,
} from "@solana/web3.js";
import type { AccountInfo, Commitment } from "@solana/web3.js";

const TOKEN_PROGRAM_ID = new PublicKey(
  "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
);
const TOKEN_2022_PROGRAM_ID = new PublicKey(
  "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
);
const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey(
  "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
);

const DEFAULT_ENV_FILE = ".dev.vars";
const DEFAULT_KEYPAIR_PATH = resolve(homedir(), ".config/solana/id.json");
const COMMITMENT: Commitment = "confirmed";
const EPHEMERAL_ATA_LEN = 80;
const LEGACY_EPHEMERAL_ATA_LEN = 72;
const SHUTTLE_METADATA_LEN = 72;
const MAX_MULTIPLE_ACCOUNT_KEYS = 100;
const RECOVER_AND_CLOSE_SHUTTLE_TO_OWNER_DISCRIMINATOR = 32;

type Options = {
  createDestinationAtas: boolean;
  dryRun: boolean;
  envFile: string;
  help?: boolean;
  limit?: number;
  mint?: PublicKey;
  payerPath?: string;
  rpcUrl?: string;
  skipSimulation: boolean;
  owner?: PublicKey;
};

type ProgramAccount = {
  account: AccountInfo<Buffer>;
  pubkey: PublicKey;
};

type EphemeralAtaAccount = {
  amount: bigint;
  mint: PublicKey;
  owner: PublicKey;
  pubkey: PublicKey;
};

type ShuttleMetadata = {
  bump: number;
  id: number;
  owner: PublicKey;
  payer: PublicKey;
};

type RecoveryCandidate = {
  amount: bigint;
  createDestinationAta: boolean;
  destinationAta: PublicKey;
  mint: PublicKey;
  owner: PublicKey;
  payer: PublicKey;
  shuttle: PublicKey;
  shuttleEata: PublicKey;
  shuttleWalletAta: PublicKey;
  tokenProgram: PublicKey;
  vault: PublicKey;
  vaultAta: PublicKey;
};

function usage() {
  return [
    "Usage:",
    "  yarn recover:shuttles -- [options]",
    "",
    "Options:",
    "  --rpc-url <url>              Base RPC URL. Defaults to BASE_RPC_URL, RPC_URL, or SOLANA_RPC_URL",
    `  --env-file <path>            Env file to load when process env is unset. Default: ${DEFAULT_ENV_FILE}`,
    `  --payer <path>               Fee payer keypair JSON path. Default: ${DEFAULT_KEYPAIR_PATH}`,
    "  --keypair <path>             Alias for --payer",
    "  --dry-run                   Simulate only. This is the default",
    "  --execute                   Send recovery transactions",
    "  --create-destination-atas    Create missing owner destination ATAs idempotently",
    "  --owner <pubkey>             Only process shuttles for this owner",
    "  --mint <pubkey>              Only process shuttles for this mint",
    "  --limit <n>                  Limit recoverable candidates after filtering",
    "  --skip-simulation            In execute mode, send without a simulation pre-check",
    "  --help                      Show this help",
  ].join("\n");
}

function parseArgs(argv: string[]): Options {
  const options: Options = {
    createDestinationAtas: false,
    dryRun: true,
    envFile: DEFAULT_ENV_FILE,
    skipSimulation: false,
  };

  let sawDryRun = false;
  let sawExecute = false;
  let index = 0;

  while (index < argv.length) {
    const arg = argv[index];

    if (arg === "--help") {
      options.help = true;
      index += 1;
      continue;
    }

    if (arg === "--dry-run") {
      sawDryRun = true;
      options.dryRun = true;
      index += 1;
      continue;
    }

    if (arg === "--execute") {
      sawExecute = true;
      options.dryRun = false;
      index += 1;
      continue;
    }

    if (arg === "--create-destination-atas") {
      options.createDestinationAtas = true;
      index += 1;
      continue;
    }

    if (arg === "--skip-simulation") {
      options.skipSimulation = true;
      index += 1;
      continue;
    }

    const nextValue = argv[index + 1];

    if (typeof nextValue === "undefined") {
      throw new Error(`Missing value for ${arg}`);
    }

    switch (arg) {
      case "--rpc-url":
        options.rpcUrl = nextValue;
        break;
      case "--env-file":
        options.envFile = nextValue;
        break;
      case "--payer":
      case "--keypair":
        options.payerPath = nextValue;
        break;
      case "--owner":
        options.owner = new PublicKey(nextValue);
        break;
      case "--mint":
        options.mint = new PublicKey(nextValue);
        break;
      case "--limit":
        options.limit = parsePositiveInt(nextValue, arg);
        break;
      default:
        throw new Error(`Unknown argument: ${arg}`);
    }

    index += 2;
  }

  if (sawDryRun && sawExecute) {
    throw new Error("Use either --dry-run or --execute, not both");
  }

  return options;
}

function parsePositiveInt(value: string, arg: string) {
  const parsed = Number(value);

  if (!Number.isInteger(parsed) || parsed <= 0) {
    throw new Error(`${arg} must be a positive integer`);
  }

  return parsed;
}

function stripWrappingQuotes(value: string) {
  if (
    (value.startsWith("\"") && value.endsWith("\""))
    || (value.startsWith("'") && value.endsWith("'"))
  ) {
    return value.slice(1, -1);
  }

  return value;
}

function parseEnvFile(filePath: string) {
  if (!existsSync(filePath)) {
    return {};
  }

  const env: Record<string, string> = {};
  const content = readFileSync(filePath, "utf8");

  for (const rawLine of content.split(/\r?\n/)) {
    const line = rawLine.trim();

    if (!line || line.startsWith("#")) {
      continue;
    }

    const separatorIndex = line.indexOf("=");

    if (separatorIndex <= 0) {
      continue;
    }

    const key = line.slice(0, separatorIndex).trim();
    const value = line.slice(separatorIndex + 1).trim();

    env[key] = stripWrappingQuotes(value);
  }

  return env;
}

function resolveRpcUrl(options: Options, env: Record<string, string | undefined>) {
  const rpcUrl = options.rpcUrl ?? env.BASE_RPC_URL ?? env.RPC_URL ?? env.SOLANA_RPC_URL;

  if (!rpcUrl) {
    throw new Error("Missing base RPC URL. Set BASE_RPC_URL or pass --rpc-url");
  }

  return rpcUrl;
}

function readKeypair(path: string) {
  const resolvedPath = resolve(path);
  const raw = readFileSync(resolvedPath, "utf8");
  const secretKey = Uint8Array.from(JSON.parse(raw));
  return Keypair.fromSecretKey(secretKey);
}

function resolvePayer(options: Options) {
  if (options.payerPath) {
    return {
      keypair: readKeypair(options.payerPath),
      source: resolve(options.payerPath),
    };
  }

  if (existsSync(DEFAULT_KEYPAIR_PATH)) {
    return {
      keypair: readKeypair(DEFAULT_KEYPAIR_PATH),
      source: DEFAULT_KEYPAIR_PATH,
    };
  }

  if (!options.dryRun) {
    throw new Error(`Missing fee payer keypair. Pass --payer or create ${DEFAULT_KEYPAIR_PATH}`);
  }

  return {
    keypair: Keypair.generate(),
    source: "generated-dry-run",
  };
}

function decodePublicKey(data: Buffer, offset: number) {
  return new PublicKey(data.subarray(offset, offset + 32));
}

function isDefaultPublicKey(pubkey: PublicKey) {
  return pubkey.equals(PublicKey.default);
}

function deriveEphemeralAta(owner: PublicKey, mint: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [owner.toBuffer(), mint.toBuffer()],
    EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
  );
}

function deriveShuttle(owner: PublicKey, mint: PublicKey, id: number) {
  const idSeed = Buffer.alloc(4);
  idSeed.writeUInt32LE(id, 0);

  return PublicKey.findProgramAddressSync(
    [owner.toBuffer(), mint.toBuffer(), idSeed],
    EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
  );
}

function deriveVault(mint: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [mint.toBuffer()],
    EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
  );
}

function deriveAssociatedTokenAddress(owner: PublicKey, mint: PublicKey, tokenProgram: PublicKey) {
  const [ata] = PublicKey.findProgramAddressSync(
    [owner.toBuffer(), tokenProgram.toBuffer(), mint.toBuffer()],
    ASSOCIATED_TOKEN_PROGRAM_ID,
  );

  return ata;
}

function parseEphemeralAta(pubkey: PublicKey, data: Buffer): EphemeralAtaAccount | null {
  if (data.length !== EPHEMERAL_ATA_LEN && data.length !== LEGACY_EPHEMERAL_ATA_LEN) {
    return null;
  }

  const owner = decodePublicKey(data, 0);
  const mint = decodePublicKey(data, 32);

  if (isDefaultPublicKey(owner) || isDefaultPublicKey(mint)) {
    return null;
  }

  const [derived, bump] = deriveEphemeralAta(owner, mint);

  if (!derived.equals(pubkey)) {
    return null;
  }

  if (data.length === EPHEMERAL_ATA_LEN && data[72] !== bump) {
    return null;
  }

  return {
    amount: data.readBigUInt64LE(64),
    mint,
    owner,
    pubkey,
  };
}

function parseShuttleMetadata(pubkey: PublicKey, mint: PublicKey, data: Buffer): ShuttleMetadata | null {
  if (data.length !== SHUTTLE_METADATA_LEN) {
    return null;
  }

  const owner = decodePublicKey(data, 0);
  const payer = decodePublicKey(data, 32);

  if (isDefaultPublicKey(owner) || isDefaultPublicKey(payer)) {
    return null;
  }

  const id = data.readUInt32LE(64);
  const bump = data[68];
  const [derived, expectedBump] = deriveShuttle(owner, mint, id);

  if (!derived.equals(pubkey) || bump !== expectedBump) {
    return null;
  }

  return {
    bump,
    id,
    owner,
    payer,
  };
}

function parseTokenAccount(data: Buffer) {
  if (data.length < 72) {
    return null;
  }

  return {
    amount: data.readBigUInt64LE(64),
    mint: decodePublicKey(data, 0),
    owner: decodePublicKey(data, 32),
  };
}

function createAssociatedTokenAccountIdempotentInstruction(
  payer: PublicKey,
  ata: PublicKey,
  owner: PublicKey,
  mint: PublicKey,
  tokenProgram: PublicKey,
) {
  return new TransactionInstruction({
    programId: ASSOCIATED_TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: payer, isSigner: true, isWritable: true },
      { pubkey: ata, isSigner: false, isWritable: true },
      { pubkey: owner, isSigner: false, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      { pubkey: tokenProgram, isSigner: false, isWritable: false },
    ],
    data: Buffer.from([1]),
  });
}

function recoverAndCloseShuttleInstruction(candidate: RecoveryCandidate) {
  return new TransactionInstruction({
    programId: EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: candidate.payer, isSigner: false, isWritable: true },
      { pubkey: candidate.shuttle, isSigner: false, isWritable: true },
      { pubkey: candidate.shuttleEata, isSigner: false, isWritable: true },
      { pubkey: candidate.shuttleWalletAta, isSigner: false, isWritable: true },
      { pubkey: candidate.destinationAta, isSigner: false, isWritable: true },
      { pubkey: candidate.mint, isSigner: false, isWritable: false },
      { pubkey: candidate.vault, isSigner: false, isWritable: false },
      { pubkey: candidate.vaultAta, isSigner: false, isWritable: true },
      { pubkey: candidate.tokenProgram, isSigner: false, isWritable: false },
    ],
    data: Buffer.from([RECOVER_AND_CLOSE_SHUTTLE_TO_OWNER_DISCRIMINATOR]),
  });
}

function buildRecoveryTransaction(candidate: RecoveryCandidate, payer: PublicKey) {
  const transaction = new Transaction();

  if (candidate.createDestinationAta) {
    transaction.add(
      createAssociatedTokenAccountIdempotentInstruction(
        payer,
        candidate.destinationAta,
        candidate.owner,
        candidate.mint,
        candidate.tokenProgram,
      ),
    );
  }

  transaction.add(recoverAndCloseShuttleInstruction(candidate));
  transaction.feePayer = payer;

  return transaction;
}

function accountData(accountInfo: AccountInfo<Buffer>) {
  return Buffer.isBuffer(accountInfo.data)
    ? accountInfo.data
    : Buffer.from(accountInfo.data);
}

function uniquePubkeys(pubkeys: PublicKey[]) {
  const byAddress = new Map<string, PublicKey>();

  for (const pubkey of pubkeys) {
    byAddress.set(pubkey.toBase58(), pubkey);
  }

  return [...byAddress.values()];
}

function chunkPubkeys(pubkeys: PublicKey[]) {
  const chunks: PublicKey[][] = [];

  for (let index = 0; index < pubkeys.length; index += MAX_MULTIPLE_ACCOUNT_KEYS) {
    chunks.push(pubkeys.slice(index, index + MAX_MULTIPLE_ACCOUNT_KEYS));
  }

  return chunks;
}

async function getMultipleAccountInfoMap(connection: Connection, pubkeys: PublicKey[]) {
  const accountInfos = new Map<string, AccountInfo<Buffer> | null>();
  const unique = uniquePubkeys(pubkeys);

  for (const chunk of chunkPubkeys(unique)) {
    const infos = await connection.getMultipleAccountsInfo(chunk, COMMITMENT);

    for (const [index, pubkey] of chunk.entries()) {
      accountInfos.set(pubkey.toBase58(), infos[index] as AccountInfo<Buffer> | null);
    }
  }

  return accountInfos;
}

async function fetchProgramAccountsByDataSize(connection: Connection, dataSize: number) {
  return await connection.getProgramAccounts(EPHEMERAL_SPL_TOKEN_PROGRAM_ID, {
    commitment: COMMITMENT,
    filters: [{ dataSize }],
  }) as ProgramAccount[];
}

function supportedTokenProgram(accountInfo: AccountInfo<Buffer> | null) {
  if (!accountInfo) {
    return null;
  }

  if (accountInfo.owner.equals(TOKEN_PROGRAM_ID) || accountInfo.owner.equals(TOKEN_2022_PROGRAM_ID)) {
    return accountInfo.owner;
  }

  return null;
}

async function discoverRecoveryCandidates(connection: Connection, options: Options) {
  console.error("Fetching program-owned eATA accounts");
  const [currentEatas, legacyEatas] = await Promise.all([
    fetchProgramAccountsByDataSize(connection, EPHEMERAL_ATA_LEN),
    fetchProgramAccountsByDataSize(connection, LEGACY_EPHEMERAL_ATA_LEN),
  ]);
  const programAccounts = [...currentEatas, ...legacyEatas];
  const eatas = programAccounts
    .map(({ pubkey, account }) => parseEphemeralAta(pubkey, accountData(account)))
    .filter((eata): eata is EphemeralAtaAccount => eata !== null)
    .filter(eata => options.mint === undefined || eata.mint.equals(options.mint));
  const shuttleAccountInfos = await getMultipleAccountInfoMap(connection, eatas.map(eata => eata.owner));
  const mintAccountInfos = await getMultipleAccountInfoMap(connection, uniquePubkeys(eatas.map(eata => eata.mint)));
  const skipped = {
    invalidDestinationAta: 0,
    invalidMetadata: 0,
    invalidShuttleWalletAta: 0,
    missingDestinationAta: 0,
    nonShuttleEatas: 0,
    ownerFilter: 0,
    unsupportedMint: 0,
  };
  const candidates: RecoveryCandidate[] = [];

  for (const eata of eatas) {
    const shuttleInfo = shuttleAccountInfos.get(eata.owner.toBase58()) ?? null;

    if (!shuttleInfo || !shuttleInfo.owner.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID)) {
      skipped.nonShuttleEatas += 1;
      continue;
    }

    const shuttle = parseShuttleMetadata(eata.owner, eata.mint, accountData(shuttleInfo));

    if (!shuttle) {
      skipped.invalidMetadata += 1;
      continue;
    }

    if (options.owner !== undefined && !shuttle.owner.equals(options.owner)) {
      skipped.ownerFilter += 1;
      continue;
    }

    const tokenProgram = supportedTokenProgram(mintAccountInfos.get(eata.mint.toBase58()) ?? null);

    if (!tokenProgram) {
      skipped.unsupportedMint += 1;
      continue;
    }

    const [vault] = deriveVault(eata.mint);
    const destinationAta = deriveAssociatedTokenAddress(shuttle.owner, eata.mint, tokenProgram);
    const shuttleWalletAta = deriveAssociatedTokenAddress(eata.owner, eata.mint, tokenProgram);

    candidates.push({
      amount: eata.amount,
      createDestinationAta: false,
      destinationAta,
      mint: eata.mint,
      owner: shuttle.owner,
      payer: shuttle.payer,
      shuttle: eata.owner,
      shuttleEata: eata.pubkey,
      shuttleWalletAta,
      tokenProgram,
      vault,
      vaultAta: deriveAssociatedTokenAddress(vault, eata.mint, tokenProgram),
    });
  }

  const shuttleWalletInfos = await getMultipleAccountInfoMap(
    connection,
    candidates.map(candidate => candidate.shuttleWalletAta),
  );
  const destinationInfos = await getMultipleAccountInfoMap(
    connection,
    candidates.map(candidate => candidate.destinationAta),
  );
  const recoverableCandidates: RecoveryCandidate[] = [];
  let existingShuttleWallets = 0;

  for (const candidate of candidates) {
    const shuttleWalletInfo = shuttleWalletInfos.get(candidate.shuttleWalletAta.toBase58()) ?? null;

    if (shuttleWalletInfo) {
      existingShuttleWallets += 1;

      if (!shuttleWalletInfo.owner.equals(candidate.tokenProgram)) {
        skipped.invalidShuttleWalletAta += 1;
        continue;
      }

      const shuttleWallet = parseTokenAccount(accountData(shuttleWalletInfo));

      if (
        !shuttleWallet
        || !shuttleWallet.mint.equals(candidate.mint)
        || !shuttleWallet.owner.equals(candidate.shuttle)
      ) {
        skipped.invalidShuttleWalletAta += 1;
        continue;
      }
    }

    const destinationInfo = destinationInfos.get(candidate.destinationAta.toBase58()) ?? null;

    if (!destinationInfo) {
      if (!options.createDestinationAtas) {
        skipped.missingDestinationAta += 1;
        continue;
      }

      recoverableCandidates.push({
        ...candidate,
        createDestinationAta: true,
      });
      continue;
    }

    if (!destinationInfo.owner.equals(candidate.tokenProgram)) {
      skipped.invalidDestinationAta += 1;
      continue;
    }

    const destination = parseTokenAccount(accountData(destinationInfo));

    if (
      !destination
      || !destination.mint.equals(candidate.mint)
      || !destination.owner.equals(candidate.owner)
    ) {
      skipped.invalidDestinationAta += 1;
      continue;
    }

    recoverableCandidates.push(candidate);
  }

  const limitedCandidates = typeof options.limit === "number"
    ? recoverableCandidates.slice(0, options.limit)
    : recoverableCandidates;

  return {
    candidates: limitedCandidates,
    existingShuttleWallets,
    scanned: {
      eataAccounts: programAccounts.length,
      parsedEatas: eatas.length,
      shuttleCandidates: candidates.length,
    },
    skipped,
    totalBeforeLimit: recoverableCandidates.length,
  };
}

async function simulateTransaction(connection: Connection, transaction: Transaction) {
  transaction.recentBlockhash = (await connection.getLatestBlockhash(COMMITMENT)).blockhash;
  const response = await connection.simulateTransaction(
    new VersionedTransaction(transaction.compileMessage()),
    {
      commitment: COMMITMENT,
      replaceRecentBlockhash: true,
      sigVerify: false,
    },
  );

  return {
    err: response.value.err,
    logs: response.value.logs ?? [],
  };
}

async function sendTransaction(connection: Connection, transaction: Transaction, payer: Keypair) {
  const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash(COMMITMENT);
  transaction.recentBlockhash = blockhash;
  transaction.sign(payer);

  const signature = await connection.sendRawTransaction(transaction.serialize(), {
    preflightCommitment: COMMITMENT,
  });
  const confirmation = await connection.confirmTransaction({
    blockhash,
    lastValidBlockHeight,
    signature,
  }, COMMITMENT);

  return {
    err: confirmation.value.err,
    signature,
  };
}

function candidateSummary(candidate: RecoveryCandidate) {
  return {
    amount: candidate.amount.toString(),
    createdDestinationAta: candidate.createDestinationAta,
    destinationAta: candidate.destinationAta.toBase58(),
    mint: candidate.mint.toBase58(),
    owner: candidate.owner.toBase58(),
    payer: candidate.payer.toBase58(),
    shuttle: candidate.shuttle.toBase58(),
    shuttleEata: candidate.shuttleEata.toBase58(),
    shuttleWalletAta: candidate.shuttleWalletAta.toBase58(),
    tokenProgram: candidate.tokenProgram.toBase58(),
    vault: candidate.vault.toBase58(),
    vaultAta: candidate.vaultAta.toBase58(),
  };
}

function formatError(error: unknown) {
  if (error === null || typeof error === "undefined") {
    return null;
  }

  if (typeof error === "string") {
    return error;
  }

  return JSON.stringify(error);
}

async function main() {
  const options = parseArgs(process.argv.slice(2));

  if (options.help) {
    console.log(usage());
    return;
  }

  const env = {
    ...parseEnvFile(resolve(process.cwd(), options.envFile)),
    ...process.env,
  };
  const rpcUrl = resolveRpcUrl(options, env);
  const payer = resolvePayer(options);
  const connection = new Connection(rpcUrl, COMMITMENT);
  const discovery = await discoverRecoveryCandidates(connection, options);

  console.error(
    `Found ${discovery.candidates.length} recoverable shuttle(s); existingShuttleWallets=${discovery.existingShuttleWallets}`,
  );

  const results = [];

  for (const candidate of discovery.candidates) {
    const transaction = buildRecoveryTransaction(candidate, payer.keypair.publicKey);
    const summary = candidateSummary(candidate);

    if (options.dryRun || !options.skipSimulation) {
      const simulation = await simulateTransaction(connection, transaction);

      if (simulation.err !== null) {
        results.push({
          ...summary,
          logs: simulation.logs.slice(-10),
          mode: "simulation",
          ok: false,
          simulationError: formatError(simulation.err),
        });

        if (!options.dryRun) {
          continue;
        }
      } else if (options.dryRun) {
        results.push({
          ...summary,
          logs: simulation.logs.slice(-10),
          mode: "simulation",
          ok: true,
        });
      }
    }

    if (!options.dryRun) {
      const sent = await sendTransaction(connection, transaction, payer.keypair);
      results.push({
        ...summary,
        confirmationError: formatError(sent.err),
        mode: "execute",
        ok: sent.err === null,
        signature: sent.signature,
      });
    }
  }

  const successful = results.filter(result => result.ok).length;
  const failed = results.length - successful;

  console.log(JSON.stringify({
    createDestinationAtas: options.createDestinationAtas,
    dryRun: options.dryRun,
    existingShuttleWallets: discovery.existingShuttleWallets,
    failed,
    payer: payer.keypair.publicKey.toBase58(),
    payerSource: payer.source,
    recoverableCandidates: discovery.candidates.length,
    results,
    rpcUrl,
    scanned: discovery.scanned,
    skipped: discovery.skipped,
    successful,
    totalBeforeLimit: discovery.totalBeforeLimit,
  }, null, 2));
}

main().catch((error: unknown) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
