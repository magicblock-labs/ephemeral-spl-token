import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";

import {
  AddressLookupTableProgram,
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import {
  DELEGATION_PROGRAM_ID,
  EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
  PERMISSION_PROGRAM_ID,
  delegateBufferPdaFromDelegatedAccountAndOwnerProgram,
  delegationMetadataPdaFromDelegatedAccount,
  delegationRecordPdaFromDelegatedAccount,
  deriveEphemeralAta,
  deriveRentPda,
  deriveTransferQueue,
  deriveVault,
  deriveVaultAta,
} from "@magicblock-labs/ephemeral-rollups-sdk";

const TOKEN_PROGRAM_ID = new PublicKey(
  "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
);
const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey(
  "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
);
const MEMO_PROGRAM_ID = new PublicKey(
  "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr",
);
const NOOP_PROGRAM_ID = new PublicKey(
  "noopb9bkMVfRPU8AsbpTUg8AQkHtKwMYZiFUjNRtMmV",
);
const WRAPPED_SOL_MINT = new PublicKey(
  "So11111111111111111111111111111111111111112",
);
const MAINNET_USDC_MINT = new PublicKey(
  "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
);
const DEVNET_USDC_MINT = new PublicKey(
  "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU",
);
const QUEUE_REFILL_STATE_SEED = Buffer.from("queue-refill");
const LAMPORTS_PDA_SEED = Buffer.from("lamports");
const DEFAULT_ENV_FILE = ".dev.vars";
const DEFAULT_KEYPAIR_PATH = resolve(homedir(), ".config/solana/id.json");
const MAX_EXTEND_ADDRESSES = 20;

function usage() {
  return [
    "Usage:",
    "  yarn create:private-transfer-lut -- [options]",
    "",
    "Options:",
    "  --cluster <mainnet|devnet>   Cluster to target. Default: mainnet",
    `  --env-file <path>            Env file to load when process env is unset. Default: ${DEFAULT_ENV_FILE}`,
    `  --payer <path>               Payer keypair JSON path. Default: ${DEFAULT_KEYPAIR_PATH}`,
    "  --authority <path>           LUT authority keypair JSON path. Default: payer",
    "  --validator <pubkey>         Validator pubkey. Defaults to getIdentity on the selected ephemeral RPC",
    "  --base-rpc-url <url>         Override the selected base RPC URL",
    "  --ephemeral-rpc-url <url>    Override the selected ephemeral RPC URL",
    "  --freeze                     Freeze the LUT after extending it",
    "  --help                       Show this help",
  ].join("\n");
}

function parseArgs(argv) {
  const options = {
    cluster: "mainnet",
    envFile: DEFAULT_ENV_FILE,
    payerPath: DEFAULT_KEYPAIR_PATH,
    authorityPath: undefined,
    validator: undefined,
    baseRpcUrl: undefined,
    ephemeralRpcUrl: undefined,
    freeze: false,
  };

  let index = 0;

  while (index < argv.length) {
    const arg = argv[index];

    if (arg === "--help") {
      options.help = true;
      index += 1;
      continue;
    }

    if (arg === "--freeze") {
      options.freeze = true;
      index += 1;
      continue;
    }

    const nextValue = argv[index + 1];

    if (typeof nextValue === "undefined") {
      throw new Error(`Missing value for ${arg}`);
    }

    switch (arg) {
      case "--cluster":
        options.cluster = nextValue;
        break;
      case "--env-file":
        options.envFile = nextValue;
        break;
      case "--payer":
        options.payerPath = nextValue;
        break;
      case "--authority":
        options.authorityPath = nextValue;
        break;
      case "--validator":
        options.validator = nextValue;
        break;
      case "--base-rpc-url":
        options.baseRpcUrl = nextValue;
        break;
      case "--ephemeral-rpc-url":
        options.ephemeralRpcUrl = nextValue;
        break;
      default:
        throw new Error(`Unknown argument: ${arg}`);
    }

    index += 2;
  }

  if (!["mainnet", "devnet"].includes(options.cluster)) {
    throw new Error("cluster must be either \"mainnet\" or \"devnet\"");
  }

  return options;
}

function stripWrappingQuotes(value) {
  if (
    (value.startsWith("\"") && value.endsWith("\""))
    || (value.startsWith("'") && value.endsWith("'"))
  ) {
    return value.slice(1, -1);
  }

  return value;
}

function parseEnvFile(filePath) {
  if (!existsSync(filePath)) {
    return {};
  }

  const env = {};
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

function readRequiredEnv(env, key) {
  const value = env[key];

  if (!value) {
    throw new Error(`Missing required environment value: ${key}`);
  }

  return value;
}

function resolveRpcUrls(options, env) {
  if (options.cluster === "devnet") {
    return {
      baseRpcUrl: options.baseRpcUrl ?? readRequiredEnv(env, "BASE_DEVNET_RPC_URL"),
      ephemeralRpcUrl:
        options.ephemeralRpcUrl ?? readRequiredEnv(env, "EPHEMERAL_DEVNET_RPC_URL"),
    };
  }

  return {
    baseRpcUrl: options.baseRpcUrl ?? readRequiredEnv(env, "BASE_RPC_URL"),
    ephemeralRpcUrl: options.ephemeralRpcUrl ?? readRequiredEnv(env, "EPHEMERAL_RPC_URL"),
  };
}

function readKeypair(path) {
  const resolvedPath = resolve(path);
  const raw = readFileSync(resolvedPath, "utf8");
  const secretKey = Uint8Array.from(JSON.parse(raw));
  return Keypair.fromSecretKey(secretKey);
}

async function resolveValidator(ephemeralRpcUrl, validatorOverride) {
  if (validatorOverride) {
    return new PublicKey(validatorOverride);
  }

  const response = await fetch(ephemeralRpcUrl, {
    method: "POST",
    headers: {
      "content-type": "application/json",
    },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "getIdentity",
      params: [],
    }),
  });

  if (!response.ok) {
    throw new Error(`Failed to resolve validator identity: HTTP ${response.status}`);
  }

  const payload = await response.json();
  const identity = payload?.result?.identity;

  if (typeof identity !== "string" || identity.length === 0) {
    throw new Error("Failed to resolve validator identity from ephemeral RPC");
  }

  return new PublicKey(identity);
}

function getMintConfigs(cluster) {
  return [
    {
      label: "sol",
      mint: WRAPPED_SOL_MINT,
    },
    {
      label: "usdc",
      mint: cluster === "devnet" ? DEVNET_USDC_MINT : MAINNET_USDC_MINT,
    },
  ];
}

function createEntry(label, pubkey) {
  return { label, pubkey };
}

function buildMintEntries(label, mint, validator) {
  const [queue] = deriveTransferQueue(mint, validator);
  const [vault] = deriveVault(mint);
  const vaultAta = deriveVaultAta(mint, vault);
  const [vaultEphemeralAta] = deriveEphemeralAta(vault, mint);
  const [rentPda] = deriveRentPda();
  const [refillState] = PublicKey.findProgramAddressSync(
    [QUEUE_REFILL_STATE_SEED, queue.toBuffer()],
    EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
  );
  const [lamportsPda] = PublicKey.findProgramAddressSync(
    [LAMPORTS_PDA_SEED, rentPda.toBuffer(), queue.toBuffer(), queue.toBuffer()],
    EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
  );

  return [
    createEntry(`${label}.mint`, mint),
    createEntry(`${label}.vault`, vault),
    createEntry(`${label}.vaultAta`, vaultAta),
    createEntry(`${label}.vaultEphemeralAta`, vaultEphemeralAta),
    createEntry(`${label}.queue`, queue),
    createEntry(`${label}.refillState`, refillState),
    createEntry(`${label}.lamportsPda`, lamportsPda),
    createEntry(
      `${label}.lamportsDelegateBuffer`,
      delegateBufferPdaFromDelegatedAccountAndOwnerProgram(
        lamportsPda,
        EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
      ),
    ),
    createEntry(
      `${label}.lamportsDelegationRecord`,
      delegationRecordPdaFromDelegatedAccount(lamportsPda),
    ),
    createEntry(
      `${label}.lamportsDelegationMetadata`,
      delegationMetadataPdaFromDelegatedAccount(lamportsPda),
    ),
    createEntry(
      `${label}.queueDelegationRecord`,
      delegationRecordPdaFromDelegatedAccount(queue),
    ),
    createEntry(
      `${label}.vaultDelegateBuffer`,
      delegateBufferPdaFromDelegatedAccountAndOwnerProgram(
        vaultEphemeralAta,
        EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
      ),
    ),
    createEntry(
      `${label}.vaultDelegationRecord`,
      delegationRecordPdaFromDelegatedAccount(vaultEphemeralAta),
    ),
    createEntry(
      `${label}.vaultDelegationMetadata`,
      delegationMetadataPdaFromDelegatedAccount(vaultEphemeralAta),
    ),
  ];
}

function buildSharedEntries() {
  const [rentPda] = deriveRentPda();

  return [
    createEntry("shared.rentPda", rentPda),
    createEntry("shared.ephemeralSplTokenProgram", EPHEMERAL_SPL_TOKEN_PROGRAM_ID),
    createEntry("shared.delegationProgram", DELEGATION_PROGRAM_ID),
    createEntry("shared.permissionProgram", PERMISSION_PROGRAM_ID),
    createEntry("shared.associatedTokenProgram", ASSOCIATED_TOKEN_PROGRAM_ID),
    createEntry("shared.tokenProgram", TOKEN_PROGRAM_ID),
    createEntry("shared.systemProgram", SystemProgram.programId),
    createEntry("shared.memoProgram", MEMO_PROGRAM_ID),
    createEntry("shared.noopProgram", NOOP_PROGRAM_ID),
  ];
}

function dedupeEntries(entries) {
  const byAddress = new Map();

  for (const entry of entries) {
    const address = entry.pubkey.toBase58();
    const current = byAddress.get(address);

    if (current) {
      current.labels.push(entry.label);
      continue;
    }

    byAddress.set(address, {
      pubkey: entry.pubkey,
      labels: [entry.label],
    });
  }

  return [...byAddress.values()];
}

function getSignerSet(payer, authority) {
  const signers = [payer];

  if (!authority.publicKey.equals(payer.publicKey)) {
    signers.push(authority);
  }

  return signers;
}

async function sendSingleInstruction(connection, instruction, signers) {
  const transaction = new Transaction().add(instruction);
  return sendAndConfirmTransaction(connection, transaction, signers, {
    commitment: "confirmed",
    preflightCommitment: "confirmed",
  });
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
  const { baseRpcUrl, ephemeralRpcUrl } = resolveRpcUrls(options, env);
  const payer = readKeypair(options.payerPath);
  const authority = readKeypair(options.authorityPath ?? options.payerPath);
  const validator = await resolveValidator(ephemeralRpcUrl, options.validator);
  const mintConfigs = getMintConfigs(options.cluster);
  const entries = dedupeEntries([
    ...buildSharedEntries(),
    ...mintConfigs.flatMap(({ label, mint }) => buildMintEntries(label, mint, validator)),
  ]);
  const addresses = entries.map(entry => entry.pubkey);
  const connection = new Connection(baseRpcUrl, "confirmed");
  const recentSlot = await connection.getSlot("finalized");
  const signers = getSignerSet(payer, authority);
  const [createInstruction, lookupTableAddress]
    = AddressLookupTableProgram.createLookupTable({
      authority: authority.publicKey,
      payer: payer.publicKey,
      recentSlot,
    });
  const summary = {
    status: "prepared",
    cluster: options.cluster,
    baseRpcUrl,
    ephemeralRpcUrl,
    validator: validator.toBase58(),
    lookupTable: lookupTableAddress.toBase58(),
    frozen: false,
    addressCount: entries.length,
    mints: mintConfigs.map(({ label, mint }) => ({
      label,
      mint: mint.toBase58(),
    })),
    addresses: entries.map(entry => ({
      address: entry.pubkey.toBase58(),
      labels: entry.labels,
    })),
  };

  console.error(`Prepared lookup table ${lookupTableAddress.toBase58()} at slot ${recentSlot}`);

  try {
    await sendSingleInstruction(connection, createInstruction, signers);
    summary.status = "created";

    for (let index = 0; index < addresses.length; index += MAX_EXTEND_ADDRESSES) {
      const chunk = addresses.slice(index, index + MAX_EXTEND_ADDRESSES);
      const extendInstruction = AddressLookupTableProgram.extendLookupTable({
        payer: payer.publicKey,
        authority: authority.publicKey,
        lookupTable: lookupTableAddress,
        addresses: chunk,
      });

      await sendSingleInstruction(connection, extendInstruction, signers);
    }

    summary.status = "extended";

    const lookupTableResponse = await connection.getAddressLookupTable(lookupTableAddress);
    const lookupTable = lookupTableResponse.value;

    if (!lookupTable) {
      throw new Error("Lookup table account was not found after creation");
    }

    const loadedAddresses = new Set(
      lookupTable.state.addresses.map(address => address.toBase58()),
    );

    for (const entry of entries) {
      if (!loadedAddresses.has(entry.pubkey.toBase58())) {
        throw new Error(`Lookup table is missing address ${entry.pubkey.toBase58()}`);
      }
    }

    if (options.freeze) {
      const freezeInstruction = AddressLookupTableProgram.freezeLookupTable({
        authority: authority.publicKey,
        lookupTable: lookupTableAddress,
      });

      await sendSingleInstruction(connection, freezeInstruction, signers);
      summary.status = "frozen";
      summary.frozen = true;
    }

    console.log(JSON.stringify(summary, null, 2));
  } catch (error) {
    summary.status = "failed";
    console.error(JSON.stringify(summary, null, 2));
    throw error;
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
