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
  deriveQueueEphemeralAta,
  deriveQueueVaultAta,
  deriveRentPda,
  deriveTransferQueue,
  deriveVault,
  deriveVaultAta,
  permissionPdaFromAccount,
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
const MAINNET_USDT_MINT = new PublicKey(
  "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
);
const DEVNET_USDT_MINT = new PublicKey(
  "BQnB36y4tTb9K1fTXkU71Q8WQa4HSNNqYw4T3agViD1y",
);
const QUEUE_REFILL_STATE_SEED = Buffer.from("queue-refill");
const LAMPORTS_PDA_SEED = Buffer.from("lamports");
const DEFAULT_ENV_FILE = ".dev.vars";
const DEFAULT_KEYPAIR_PATH = resolve(homedir(), ".config/solana/id.json");
const MAX_EXTEND_ADDRESSES = 20;
const MAX_LOOKUP_TABLE_ADDRESSES = 256;
const CLUSTERS = ["mainnet", "devnet", "mainnet-private", "devnet-private"];

function usage() {
  return [
    "Usage:",
    "  yarn create:private-transfer-lut -- [options]",
    "",
    "Options:",
    "  --cluster <mainnet|devnet>   Base cluster to target. Default: mainnet",
    `  --env-file <path>            Env file to load when process env is unset. Default: ${DEFAULT_ENV_FILE}`,
    `  --payer <path>               Payer keypair JSON path. Default: ${DEFAULT_KEYPAIR_PATH}`,
    "  --authority <path>           LUT authority keypair JSON path. Default: payer",
    "  --validator <pubkey[,pubkey]>",
    "                               Validator pubkey(s). May be repeated. Defaults to getIdentity on base and TEE ephemeral RPCs",
    "  --base-rpc-url <url>         Override the selected base RPC URL",
    "  --ephemeral-rpc-url <url[,url]>",
    "                               Override ephemeral RPC URL(s) used for validator resolution. May be repeated",
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
    validators: [],
    baseRpcUrl: undefined,
    ephemeralRpcUrls: [],
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
        options.validators.push(...parseList(nextValue, arg));
        break;
      case "--base-rpc-url":
        options.baseRpcUrl = nextValue;
        break;
      case "--ephemeral-rpc-url":
        options.ephemeralRpcUrls.push(...parseList(nextValue, arg));
        break;
      default:
        throw new Error(`Unknown argument: ${arg}`);
    }

    index += 2;
  }

  if (!CLUSTERS.includes(options.cluster)) {
    throw new Error(
      "cluster must be \"mainnet\", \"devnet\", \"mainnet-private\", or \"devnet-private\"",
    );
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

function parseList(value, arg) {
  const values = value
    .split(",")
    .map(part => part.trim())
    .filter(Boolean);

  if (values.length === 0) {
    throw new Error(`Missing value for ${arg}`);
  }

  return values;
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

function getBaseCluster(cluster) {
  return cluster === "devnet" || cluster === "devnet-private" ? "devnet" : "mainnet";
}

function resolveBaseRpcUrl(options, env, baseCluster) {
  if (baseCluster === "devnet") {
    return options.baseRpcUrl ?? readRequiredEnv(env, "BASE_DEVNET_RPC_URL");
  }

  return options.baseRpcUrl ?? readRequiredEnv(env, "BASE_RPC_URL");
}

function dedupeRpcConfigs(configs) {
  const byUrl = new Map();

  for (const config of configs) {
    const current = byUrl.get(config.url);

    if (current) {
      current.labels.push(config.label);
      continue;
    }

    byUrl.set(config.url, {
      labels: [config.label],
      url: config.url,
    });
  }

  return [...byUrl.values()].map(config => ({
    label: config.labels.join("+"),
    url: config.url,
  }));
}

function resolveEphemeralRpcConfigs(options, env, baseCluster) {
  if (options.ephemeralRpcUrls.length > 0) {
    return dedupeRpcConfigs(
      options.ephemeralRpcUrls.map((url, index) => ({
        label: `rpc${index + 1}`,
        url,
      })),
    );
  }

  if (baseCluster === "devnet") {
    return dedupeRpcConfigs([
      {
        label: "ephemeral",
        url: readRequiredEnv(env, "EPHEMERAL_DEVNET_RPC_URL"),
      },
      {
        label: "tee",
        url: readRequiredEnv(env, "EPHEMERAL_DEVNET_TEE_RPC_URL"),
      },
    ]);
  }

  return dedupeRpcConfigs([
    {
      label: "ephemeral",
      url: readRequiredEnv(env, "EPHEMERAL_RPC_URL"),
    },
    {
      label: "tee",
      url: readRequiredEnv(env, "EPHEMERAL_TEE_RPC_URL"),
    },
  ]);
}

function readKeypair(path) {
  const resolvedPath = resolve(path);
  const raw = readFileSync(resolvedPath, "utf8");
  const secretKey = Uint8Array.from(JSON.parse(raw));
  return Keypair.fromSecretKey(secretKey);
}

async function resolveValidatorFromRpc({ label, url }) {
  const response = await fetch(url, {
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
    throw new Error(`Failed to resolve ${label} validator identity: HTTP ${response.status}`);
  }

  const payload = await response.json();
  const identity = payload?.result?.identity;

  if (typeof identity !== "string" || identity.length === 0) {
    throw new Error(`Failed to resolve ${label} validator identity from ephemeral RPC`);
  }

  return {
    label,
    pubkey: new PublicKey(identity),
  };
}

function dedupeValidatorConfigs(configs) {
  const byAddress = new Map();

  for (const config of configs) {
    const address = config.pubkey.toBase58();
    const current = byAddress.get(address);

    if (current) {
      current.labels.push(config.label);
      current.rpcUrls.push(...config.rpcUrls);
      continue;
    }

    byAddress.set(address, {
      labels: [config.label],
      pubkey: config.pubkey,
      rpcUrls: config.rpcUrls,
    });
  }

  return [...byAddress.values()].map(config => ({
    label: config.labels.join("+"),
    pubkey: config.pubkey,
    rpcUrls: [...new Set(config.rpcUrls)],
  }));
}

async function resolveValidators(options, ephemeralRpcConfigs) {
  if (options.validators.length > 0) {
    return dedupeValidatorConfigs(
      options.validators.map((validator, index) => ({
        label: `validator${index + 1}`,
        pubkey: new PublicKey(validator),
        rpcUrls: [],
      })),
    );
  }

  const resolvedValidators = await Promise.all(
    ephemeralRpcConfigs.map(async (config) => {
      const validator = await resolveValidatorFromRpc(config);
      return {
        ...validator,
        rpcUrls: [config.url],
      };
    }),
  );

  return dedupeValidatorConfigs(resolvedValidators);
}

function getMintConfigs(cluster) {
  const baseCluster = getBaseCluster(cluster);

  return [
    {
      label: "sol",
      mint: WRAPPED_SOL_MINT,
    },
    {
      label: "usdc",
      mint: baseCluster === "devnet" ? DEVNET_USDC_MINT : MAINNET_USDC_MINT,
    },
    {
      label: "usdt",
      mint: baseCluster === "devnet" ? DEVNET_USDT_MINT : MAINNET_USDT_MINT,
    },
  ];
}

function createEntry(label, pubkey) {
  return { label, pubkey };
}

function buildMintEntries(label, mint, validator) {
  const [queue] = deriveTransferQueue(mint, validator);
  const queueAta = deriveQueueVaultAta(mint, validator);
  const [queueEphemeralAta] = deriveQueueEphemeralAta(mint, validator);
  const queuePermission = permissionPdaFromAccount(queue);
  const queueEphemeralAtaPermission = permissionPdaFromAccount(queueEphemeralAta);
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
    createEntry(`${label}.queueAta`, queueAta),
    createEntry(`${label}.queueEphemeralAta`, queueEphemeralAta),
    createEntry(`${label}.queuePermission`, queuePermission),
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
      `${label}.queueDelegateBuffer`,
      delegateBufferPdaFromDelegatedAccountAndOwnerProgram(
        queue,
        EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
      ),
    ),
    createEntry(
      `${label}.queueDelegationRecord`,
      delegationRecordPdaFromDelegatedAccount(queue),
    ),
    createEntry(
      `${label}.queueDelegationMetadata`,
      delegationMetadataPdaFromDelegatedAccount(queue),
    ),
    createEntry(
      `${label}.queueEphemeralAtaDelegateBuffer`,
      delegateBufferPdaFromDelegatedAccountAndOwnerProgram(
        queueEphemeralAta,
        EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
      ),
    ),
    createEntry(
      `${label}.queueEphemeralAtaDelegationRecord`,
      delegationRecordPdaFromDelegatedAccount(queueEphemeralAta),
    ),
    createEntry(
      `${label}.queueEphemeralAtaDelegationMetadata`,
      delegationMetadataPdaFromDelegatedAccount(queueEphemeralAta),
    ),
    createEntry(
      `${label}.queueEphemeralAtaPermission`,
      queueEphemeralAtaPermission,
    ),
    createEntry(
      `${label}.queueEphemeralAtaPermissionDelegateBuffer`,
      delegateBufferPdaFromDelegatedAccountAndOwnerProgram(
        queueEphemeralAtaPermission,
        PERMISSION_PROGRAM_ID,
      ),
    ),
    createEntry(
      `${label}.queueEphemeralAtaPermissionDelegationRecord`,
      delegationRecordPdaFromDelegatedAccount(queueEphemeralAtaPermission),
    ),
    createEntry(
      `${label}.queueEphemeralAtaPermissionDelegationMetadata`,
      delegationMetadataPdaFromDelegatedAccount(queueEphemeralAtaPermission),
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
  const baseCluster = getBaseCluster(options.cluster);
  const baseRpcUrl = resolveBaseRpcUrl(options, env, baseCluster);
  const ephemeralRpcConfigs = options.validators.length > 0
    ? []
    : resolveEphemeralRpcConfigs(options, env, baseCluster);
  const payer = readKeypair(options.payerPath);
  const authority = readKeypair(options.authorityPath ?? options.payerPath);
  const validators = await resolveValidators(options, ephemeralRpcConfigs);
  const mintConfigs = getMintConfigs(baseCluster);
  const entries = dedupeEntries([
    ...buildSharedEntries(),
    ...validators.flatMap(validator =>
      mintConfigs.flatMap(({ label, mint }) =>
        buildMintEntries(`${validator.label}.${label}`, mint, validator.pubkey),
      ),
    ),
  ]);
  const addresses = entries.map(entry => entry.pubkey);

  if (addresses.length > MAX_LOOKUP_TABLE_ADDRESSES) {
    throw new Error(
      `Lookup table would contain ${addresses.length} addresses, exceeding the ${MAX_LOOKUP_TABLE_ADDRESSES} address limit`,
    );
  }

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
    cluster: baseCluster,
    requestedCluster: options.cluster,
    baseRpcUrl,
    ephemeralRpcUrls: ephemeralRpcConfigs.map(config => ({
      label: config.label,
      url: config.url,
    })),
    validators: validators.map(validator => ({
      label: validator.label,
      pubkey: validator.pubkey.toBase58(),
      rpcUrls: validator.rpcUrls,
    })),
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
