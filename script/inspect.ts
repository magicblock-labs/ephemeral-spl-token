#!/usr/bin/env node
// Lightweight on-chain inspector for private transfer queues.
//
// Run from repo root:
//   ./inspect queue
//   ./inspect tick
//   ./inspect crank --ensure
//
// The script intentionally avoids a build step. Keep imports pointed at api/node_modules.

import { readFileSync } from "node:fs";

import web3 from "../api/node_modules/@solana/web3.js/lib/index.cjs.js";
import sdk from "../api/node_modules/@magicblock-labs/ephemeral-rollups-sdk/lib/index.js";

const { Connection, Keypair, PublicKey, Transaction, TransactionInstruction } = web3;
const {
  DELEGATION_PROGRAM_ID,
  deriveTransferQueue,
  deriveVault,
  deriveVaultAta,
  EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
  ensureTransferQueueCrankIx,
  MAGIC_CONTEXT_ID,
  MAGIC_PROGRAM_ID,
  magicFeeVaultPdaFromValidator,
} = sdk;

const HEADER_LEN = 96;
const ITEM_LEN = 96;
const LAMPORTS_PER_SOL = 1_000_000_000;
const DEFAULT_QUEUE_MINT = new PublicKey("G1yLkTzfqMzi1RhtZsvQdociimZbwh9tKjapHZVuhknh");
const DEFAULT_BASE_RPC = "http://127.0.0.1:8899";
const DEFAULT_ER_RPC = "http://127.0.0.1:7799";
const DEFAULT_PAYER_PATH = "sender.json";
const PROCESS_TRANSFER_QUEUE_TICK_DISCRIMINATOR = 203;

const TOKEN_PROGRAM_ID = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const TOKEN_2022_PROGRAM_ID = new PublicKey("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
const USE_COLOR = process.env.NO_COLOR === undefined && process.env.TERM !== "dumb";
const ANSI_RESET = "\u001b[0m";
const ANSI_BOLD = "\u001b[1m";
const ANSI_DIM = "\u001b[2m";
const ANSI_RED = "\u001b[31m";
const ANSI_GREEN = "\u001b[32m";
const ANSI_YELLOW = "\u001b[33m";
const ANSI_BLUE = "\u001b[34m";
const ANSI_MAGENTA = "\u001b[35m";
const ANSI_CYAN = "\u001b[36m";

function colorize(value, color) {
  return USE_COLOR ? `${color}${value}${ANSI_RESET}` : value;
}

function bold(value) {
  return colorize(value, ANSI_BOLD);
}

function strong(value, color = ANSI_CYAN) {
  return USE_COLOR ? `${ANSI_BOLD}${color}${value}${ANSI_RESET}` : value;
}

function dim(value) {
  return colorize(value, ANSI_DIM);
}

function red(value) {
  return colorize(value, ANSI_RED);
}

function green(value) {
  return colorize(value, ANSI_GREEN);
}

function yellow(value) {
  return colorize(value, ANSI_YELLOW);
}

function blue(value) {
  return colorize(value, ANSI_BLUE);
}

function magenta(value) {
  return colorize(value, ANSI_MAGENTA);
}

function cyan(value) {
  return colorize(value, ANSI_CYAN);
}

function section(title, color = ANSI_CYAN) {
  console.log("");
  console.log(dim("=".repeat(72)));
  console.log(`${colorize("==", color)} ${strong(title, color)}`);
  console.log(dim("=".repeat(72)));
}

function subsection(title, color = ANSI_BLUE) {
  console.log("");
  console.log(`${colorize("--", color)} ${strong(title, color)}`);
}

function keyValue(label, value, color = ANSI_BLUE) {
  console.log(`  ${colorize(label.padEnd(18), color)} ${value}`);
}

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith("--")) {
      continue;
    }

    const key = arg.slice(2);
    if (key === "help" || key === "verbose" || key === "full" || key === "ensure") {
      out[key] = true;
      continue;
    }

    const value = argv[i + 1];
    if (value === undefined || value.startsWith("--")) {
      throw new Error(`Missing value for --${key}`);
    }
    out[key] = value;
    i += 1;
  }
  return out;
}

function parseCommand(argv) {
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith("--")) {
      return arg;
    }
    if (arg !== "--help") {
      i += 1;
    }
  }
  return undefined;
}

function parseHelpTarget(argv) {
  const helpIndex = argv.indexOf("help");
  if (helpIndex < 0) {
    return undefined;
  }
  return argv.slice(helpIndex + 1).find((arg) => !arg.startsWith("--"));
}

function validateOptions(command, args) {
  const allowedByCommand = {
    queue: new Set(["help", "verbose", "full"]),
    tick: new Set(["help", "payer", "verbose", "full"]),
    crank: new Set(["help", "ensure", "payer", "verbose", "full"]),
  };
  const allowed = allowedByCommand[command] ?? new Set(["help"]);
  for (const key of Object.keys(args)) {
    if (!allowed.has(key)) {
      throw new Error(`Unknown option for ${command}: --${key}`);
    }
  }
}

function rootUsage() {
  console.log(`
${bold("Usage:")}
  ${cyan("./inspect queue [--verbose|--full]")}
  ${cyan("./inspect tick [--payer <path>] [--verbose|--full]")}
  ${cyan("./inspect crank --ensure [--payer <path>] [--verbose|--full]")}

${bold("Commands:")}
  ${blue("queue")}              Inspect ER queues and base vault balance.
                     ${dim("Flags:")} ${blue("--verbose")}, ${blue("--full")}, ${blue("--help")}
                     ${dim("--verbose prints account owners, rent, RPCs, queue header, and Magic fee vault.")}

  ${blue("tick")}               Invoke ProcessTransferQueueTick on ER.
                     ${dim("Flags:")} ${blue("--payer <path>")}, ${blue("--verbose")}, ${blue("--full")}, ${blue("--help")}
                     ${dim("--payer defaults to PAYER_KEYPAIR or sender.json.")}
                     ${dim("--verbose prints queue length and validator before sending each tick.")}

  ${blue("crank")}              Manage the recurring transfer queue crank on ER.
                     ${dim("Flags:")} ${blue("--ensure")}, ${blue("--payer <path>")}, ${blue("--verbose")}, ${blue("--full")}, ${blue("--help")}
                     ${dim("--ensure sends EnsureTransferQueueCrank.")}
                     ${dim("--payer defaults to PAYER_KEYPAIR or sender.json.")}

  ${blue("help [command]")}     Print command help.

${bold("Defaults:")}
  ${blue("ER RPC")}             ${DEFAULT_ER_RPC}
  ${blue("base RPC")}           ${DEFAULT_BASE_RPC}
  ${blue("mint")}               ${DEFAULT_QUEUE_MINT.toBase58()}

${dim("Focused help: ./inspect queue --help, ./inspect tick --help, or ./inspect crank --help")}
`);
}

function queueUsage() {
  console.log(`
${bold("Usage:")}
  ${cyan("./inspect queue [--verbose|--full]")}

${bold("Description:")}
  Inspect matching transfer queue accounts and queued items on ER, then fetch vault state from base.

${bold("Options:")}
  ${blue("--verbose")}          Print account owners, rent, RPCs, queue header, and Magic fee vault.
  ${blue("--full")}             Alias for --verbose.
  ${blue("--help")}             Print this help.

${bold("Defaults:")}
  ${blue("ER RPC")}             ${DEFAULT_ER_RPC}
  ${blue("base RPC")}           ${DEFAULT_BASE_RPC}
  ${blue("mint")}               ${DEFAULT_QUEUE_MINT.toBase58()}
`);
}

function tickUsage() {
  console.log(`
${bold("Usage:")}
  ${cyan("./inspect tick [--payer <path>] [--verbose|--full]")}

${bold("Description:")}
  Send one ProcessTransferQueueTick instruction for each matching transfer queue on ER.

${bold("Options:")}
  ${blue("--payer <path>")}     Payer keypair. Defaults to PAYER_KEYPAIR or ${DEFAULT_PAYER_PATH}.
  ${blue("--verbose")}          Print queue length and validator before sending each tick.
  ${blue("--full")}             Alias for --verbose.
  ${blue("--help")}             Print this help.

${bold("Defaults:")}
  ${blue("ER RPC")}             ${DEFAULT_ER_RPC}
  ${blue("mint")}               ${DEFAULT_QUEUE_MINT.toBase58()}
`);
}

function crankUsage() {
  console.log(`
${bold("Usage:")}
  ${cyan("./inspect crank --ensure [--payer <path>] [--verbose|--full]")}

${bold("Description:")}
  Ensure the recurring transfer queue crank is scheduled on ER.

${bold("Options:")}
  ${blue("--ensure")}           Send EnsureTransferQueueCrank.
  ${blue("--payer <path>")}     Payer keypair. Defaults to PAYER_KEYPAIR or ${DEFAULT_PAYER_PATH}.
  ${blue("--verbose")}          Print queue length, validator, and Magic fee vault before sending.
  ${blue("--full")}             Alias for --verbose.
  ${blue("--help")}             Print this help.

${bold("Instruction:")}
  ${blue("EnsureTransferQueueCrank")} discriminator ${blue("17")}

${bold("Defaults:")}
  ${blue("ER RPC")}             ${DEFAULT_ER_RPC}
  ${blue("mint")}               ${DEFAULT_QUEUE_MINT.toBase58()}
`);
}

function printHelp(command) {
  if (command === "queue") {
    queueUsage();
    return;
  }
  if (command === "tick") {
    tickUsage();
    return;
  }
  if (command === "crank") {
    crankUsage();
    return;
  }
  rootUsage();
}

function sol(lamports) {
  if (lamports === undefined || lamports === null) {
    return "n/a";
  }
  return `${(lamports / LAMPORTS_PER_SOL).toFixed(9)} SOL`;
}

function colorOwner(owner) {
  if (owner.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID)) {
    return green(owner.toBase58());
  }
  if (owner.equals(DELEGATION_PROGRAM_ID)) {
    return yellow(owner.toBase58());
  }
  if (owner.equals(TOKEN_PROGRAM_ID) || owner.equals(TOKEN_2022_PROGRAM_ID)) {
    return green(owner.toBase58());
  }
  return red(owner.toBase58());
}

function colorRentDelta(diff) {
  const text = `${diff >= 0 ? "+" : ""}${diff} (${sol(diff)})`;
  return diff >= 0 ? green(text) : red(text);
}

function colorLength(parsed) {
  const text = `${parsed.length}/${parsed.capacity}`;
  if (parsed.invalidLength) {
    return red(`${text} INVALID_LENGTH`);
  }
  if (parsed.length > 0) {
    return yellow(text);
  }
  return green(text);
}

function rawToUi(raw, decimals) {
  if (decimals === undefined || decimals === null) {
    return raw.toString();
  }

  const negative = raw < 0n;
  const value = negative ? -raw : raw;
  const scale = 10n ** BigInt(decimals);
  const whole = value / scale;
  const fraction = value % scale;
  const fractionText = fraction.toString().padStart(decimals, "0").replace(/0+$/, "");
  return `${negative ? "-" : ""}${whole.toString()}${fractionText ? `.${fractionText}` : ""}`;
}

function tokenProgramName(programId) {
  if (programId.equals(TOKEN_PROGRAM_ID)) {
    return "SPL Token";
  }
  if (programId.equals(TOKEN_2022_PROGRAM_ID)) {
    return "Token-2022";
  }
  return "unknown";
}

function tokenProgramFromKind(kind) {
  return kind === 1 ? TOKEN_2022_PROGRAM_ID : TOKEN_PROGRAM_ID;
}

function readKeypair(path) {
  const secret = JSON.parse(readFileSync(path, "utf8"));
  return Keypair.fromSecretKey(Uint8Array.from(secret));
}

function readU24LE(data, offset) {
  return data[offset] + (data[offset + 1] << 8) + (data[offset + 2] << 16);
}

function parseQueue(data) {
  if (!data || data.length < HEADER_LEN) {
    return null;
  }

  const capacity = Math.floor((data.length - HEADER_LEN) / ITEM_LEN);
  const length = data.readUInt32LE(40);
  const activeLength = Math.min(length, capacity);
  const items = [];

  for (let i = 0; i < activeLength; i += 1) {
    const offset = HEADER_LEN + i * ITEM_LEN;
    items.push({
      index: i,
      source: new PublicKey(data.subarray(offset, offset + 32)),
      destinationOwner: new PublicKey(data.subarray(offset + 32, offset + 64)),
      amount: data.readBigUInt64LE(offset + 64),
      readyAt: data.readBigInt64LE(offset + 72),
      clientRefId: data.readBigUInt64LE(offset + 80),
      taskId: data.readUInt32LE(offset + 88),
      flags: data[offset + 92],
      groupId: readU24LE(data, offset + 93),
    });
  }

  return {
    version: data[0],
    bump: data[1],
    tokenProgramKind: data[2],
    mint: new PublicKey(data.subarray(8, 40)),
    length,
    capacity,
    activeLength,
    groupId: data.readUInt32LE(44),
    nextTaskId: data.readUInt32LE(52),
    crankTaskId: data.readBigInt64LE(56),
    validator: new PublicKey(data.subarray(64, 96)),
    items,
    invalidLength: length > capacity,
  };
}

function formatTime(msBigInt) {
  const ms = Number(msBigInt);
  if (!Number.isFinite(ms)) {
    return `${msBigInt.toString()} ms`;
  }

  const date = new Date(ms);
  const deltaMs = ms - Date.now();
  const absSeconds = Math.abs(deltaMs) / 1000;
  const direction = deltaMs >= 0 ? "from now" : "ago";
  return `${date.toISOString()} (${absSeconds.toFixed(1)}s ${direction})`;
}

function decodeFlags(flags) {
  const parts = [];
  if ((flags & 1) !== 0) {
    parts.push("create-destination-ata");
  }
  const unknown = flags & ~1;
  if (unknown !== 0) {
    parts.push(`unknown:0x${unknown.toString(16)}`);
  }
  return parts.length > 0 ? parts.join(", ") : "none";
}

async function getMintDecimals(connection, mint) {
  const info = await connection.getAccountInfo(mint, "confirmed");
  if (!info || info.data.length < 45) {
    return undefined;
  }
  return info.data[44];
}

async function getTokenAmount(connection, tokenAccount) {
  try {
    return await connection.getTokenAccountBalance(tokenAccount, "confirmed");
  } catch (error) {
    return { error: error instanceof Error ? error.message : String(error) };
  }
}

async function printAccountRent(connection, label, pubkey, accountInfo) {
  if (!accountInfo) {
    console.log(`${yellow(label)}: ${red("missing")}`);
    return;
  }

  console.log(`${yellow(label)}: ${cyan(pubkey.toBase58())}`);
  keyValue("owner", colorOwner(accountInfo.owner));
  keyValue("lamports", `${magenta(String(accountInfo.lamports))} ${dim(`(${sol(accountInfo.lamports)})`)}`);
  keyValue("data len", String(accountInfo.data.length));

  try {
    const rentMin = await connection.getMinimumBalanceForRentExemption(accountInfo.data.length);
    const diff = accountInfo.lamports - rentMin;
    keyValue("rent exempt min", `${rentMin} ${dim(`(${sol(rentMin)})`)}`);
    keyValue("rent delta", colorRentDelta(diff));
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    keyValue("rent exempt min", `${yellow("unavailable")} ${dim(`(${message})`)}`);
  }
}

async function printVault(connection, label, mint, tokenProgram, decimals) {
  const [vault] = deriveVault(mint);
  const vaultAta = deriveVaultAta(mint, vault, tokenProgram);
  const vaultInfo = await connection.getAccountInfo(vault, "confirmed");
  const vaultAtaInfo = await connection.getAccountInfo(vaultAta, "confirmed");
  const vaultAmount = await getTokenAmount(connection, vaultAta);

  subsection(`Vault (${label})`, ANSI_MAGENTA);
  keyValue("RPC", cyan(connection.rpcEndpoint), ANSI_MAGENTA);
  await printAccountRent(connection, "  vault PDA", vault, vaultInfo);
  await printAccountRent(connection, "  vault ATA", vaultAta, vaultAtaInfo);
  if ("value" in vaultAmount) {
    keyValue(
      "token amount",
      `${strong(vaultAmount.value.uiAmountString, ANSI_MAGENTA)} ${dim(`(${vaultAmount.value.amount} raw, decimals=${vaultAmount.value.decimals})`)}`,
      ANSI_MAGENTA,
    );
  } else {
    keyValue("token amount", `${yellow("unavailable")} ${dim(`(${vaultAmount.error})`)}`, ANSI_MAGENTA);
    if (decimals !== undefined) {
      keyValue("mint decimals", String(decimals), ANSI_MAGENTA);
    }
  }
}

async function printVaultSummary(connection, mint, tokenProgram) {
  const [vault] = deriveVault(mint);
  const vaultAta = deriveVaultAta(mint, vault, tokenProgram);
  const vaultAmount = await getTokenAmount(connection, vaultAta);

  subsection("Vault", ANSI_MAGENTA);
  keyValue("address", cyan(vaultAta.toBase58()), ANSI_MAGENTA);
  if ("value" in vaultAmount) {
    keyValue(
      "balance",
      `${strong(vaultAmount.value.uiAmountString, ANSI_MAGENTA)} ${dim(`(${vaultAmount.value.amount} raw, decimals=${vaultAmount.value.decimals})`)}`,
      ANSI_MAGENTA,
    );
  } else {
    keyValue("balance", `${yellow("unavailable")} ${dim(`(${vaultAmount.error})`)}`, ANSI_MAGENTA);
  }
}

function printQueueHeader(queue, accountInfo, parsed, expectedQueue) {
  subsection("Queue", ANSI_CYAN);
  keyValue("address", cyan(queue.toBase58()));
  if (expectedQueue) {
    const matches = expectedQueue.equals(queue);
    keyValue(
      "expected PDA",
      `${cyan(expectedQueue.toBase58())} ${matches ? green("(match)") : red("(MISMATCH)")}`,
    );
  }

  if (!accountInfo) {
    keyValue("account", red("missing"));
    return;
  }

  keyValue("owner", colorOwner(accountInfo.owner));
  keyValue("lamports", `${magenta(String(accountInfo.lamports))} ${dim(`(${sol(accountInfo.lamports)})`)}`);
  keyValue("data len", String(accountInfo.data.length));

  if (!parsed) {
    keyValue("queue header", red("unavailable"));
    return;
  }

  const tokenProgram = tokenProgramFromKind(parsed.tokenProgramKind);
  keyValue("version", String(parsed.version));
  keyValue("bump", String(parsed.bump));
  keyValue("mint", cyan(parsed.mint.toBase58()));
  keyValue("validator", cyan(parsed.validator.toBase58()));
  keyValue("token program", `${cyan(tokenProgram.toBase58())} ${dim(`(${tokenProgramName(tokenProgram)})`)}`);
  keyValue("length/capacity", colorLength(parsed), parsed.length > 0 ? ANSI_YELLOW : ANSI_GREEN);
  keyValue("group cursor", String(parsed.groupId));
  keyValue("next task id", String(parsed.nextTaskId));
  keyValue("crank task id", parsed.crankTaskId === 0n ? dim("none") : yellow(parsed.crankTaskId.toString()));
}

async function printQueueItems(parsed, decimals, options = { verbose: false }) {
  subsection("Items", ANSI_YELLOW);
  if (!parsed || parsed.items.length === 0) {
    keyValue("status", green("empty"), ANSI_GREEN);
    return;
  }

  for (const item of parsed.items) {
    const ready = Number(item.readyAt) <= Date.now();
    const task = options.verbose ? ` task=${cyan(String(item.taskId))} group=${cyan(String(item.groupId))}` : "";
    console.log(`  ${yellow(`[${item.index}]`)}${task}`);
    keyValue("amount", `${magenta(rawToUi(item.amount, decimals))} ${dim(`(${item.amount.toString()} raw)`)}`, ANSI_MAGENTA);
    keyValue("ready", ready ? green(formatTime(item.readyAt)) : yellow(formatTime(item.readyAt)), ready ? ANSI_GREEN : ANSI_YELLOW);
    keyValue("source", cyan(item.source.toBase58()));
    keyValue("destination", cyan(item.destinationOwner.toBase58()));
    if (options.verbose) {
      keyValue("client ref id", item.clientRefId.toString());
      keyValue("flags", `${item.flags} ${dim(`(${decodeFlags(item.flags)})`)}`);
    }
  }
}

async function printQueueSummary(queue, parsed, decimals) {
  subsection("Queue", ANSI_CYAN);
  keyValue("address", cyan(queue.toBase58()));
  await printQueueItems(parsed, decimals, { verbose: false });
}

async function inspectQueue(queueConnection, vaultConnection, label, queue, expectedQueue, options) {
  if (options.verbose) {
    section(label, label.includes("base") ? ANSI_BLUE : ANSI_CYAN);
    keyValue("queue RPC", cyan(queueConnection.rpcEndpoint));
  }

  const accountInfo = await queueConnection.getAccountInfo(queue, "confirmed");
  const parsed = accountInfo ? parseQueue(accountInfo.data) : null;
  const mint = parsed?.mint;
  const validator = parsed?.validator;
  const tokenProgram = parsed ? tokenProgramFromKind(parsed.tokenProgramKind) : TOKEN_PROGRAM_ID;
  const decimals = mint ? await getMintDecimals(queueConnection, mint) : undefined;

  if (options.verbose) {
    printQueueHeader(queue, accountInfo, parsed, expectedQueue);
    if (accountInfo) {
      await printAccountRent(queueConnection, "  rent", queue, accountInfo);
    }

    if (validator) {
      subsection("Magic", ANSI_BLUE);
      keyValue("magic fee vault", cyan(magicFeeVaultPdaFromValidator(validator).toBase58()));
    }

    await printQueueItems(parsed, decimals, options);
  } else {
    await printQueueSummary(queue, parsed, decimals);
  }

  if (mint) {
    if (options.verbose) {
      await printVault(vaultConnection, "base", mint, tokenProgram, decimals);
    } else {
      await printVaultSummary(vaultConnection, mint, tokenProgram);
    }
  }
}

async function scanQueues(queueConnection, vaultConnection, label, mint, options) {
  if (options.verbose) {
    section(`${label} scan`, label.includes("base") ? ANSI_BLUE : ANSI_CYAN);
    keyValue("queue RPC", cyan(queueConnection.rpcEndpoint));
    keyValue("vault RPC", cyan(vaultConnection.rpcEndpoint));
    keyValue("mint filter", cyan(mint.toBase58()));
  } else {
    section("queue", ANSI_CYAN);
  }

  const queues = await findQueues(queueConnection, mint);
  for (let i = 0; i < queues.length; i += 1) {
    const queue = queues[i];
    await inspectQueue(queueConnection, vaultConnection, `${label} queue ${i + 1}`, queue.pubkey, queue.expectedQueue, options);
  }

  if (queues.length === 0) {
    console.log(yellow("No matching queue accounts found."));
  }
}

async function findQueues(connection, mint) {
  const ownerPrograms = [EPHEMERAL_SPL_TOKEN_PROGRAM_ID, DELEGATION_PROGRAM_ID];
  const queues = [];

  for (const ownerProgram of ownerPrograms) {
    const accounts = await connection.getProgramAccounts(ownerProgram, "confirmed");
    for (const { pubkey, account } of accounts) {
      const parsed = parseQueue(account.data);
      if (!parsed || parsed.version !== 1 || !parsed.mint.equals(mint)) {
        continue;
      }

      const [expectedQueue] = deriveTransferQueue(parsed.mint, parsed.validator);
      if (!expectedQueue.equals(pubkey)) {
        continue;
      }

      queues.push({ pubkey, account, parsed, expectedQueue });
    }
  }

  return queues;
}

async function sendTick(connection, payer, queue) {
  const magicFeeVault = magicFeeVaultPdaFromValidator(queue.parsed.validator);
  const ix = new TransactionInstruction({
    programId: EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: queue.pubkey, isSigner: false, isWritable: true },
      { pubkey: magicFeeVault, isSigner: false, isWritable: true },
      { pubkey: MAGIC_CONTEXT_ID, isSigner: false, isWritable: true },
      { pubkey: MAGIC_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: Buffer.from([PROCESS_TRANSFER_QUEUE_TICK_DISCRIMINATOR]),
  });

  const latestBlockhash = await connection.getLatestBlockhash("confirmed");
  const tx = new Transaction({
    feePayer: payer.publicKey,
    blockhash: latestBlockhash.blockhash,
    lastValidBlockHeight: latestBlockhash.lastValidBlockHeight,
  }).add(ix);
  tx.sign(payer);

  const signature = await connection.sendRawTransaction(tx.serialize(), { skipPreflight: false });
  await connection.confirmTransaction({ signature, ...latestBlockhash }, "confirmed");
  return signature;
}

async function sendEnsureCrank(connection, payer, queue) {
  const magicFeeVault = magicFeeVaultPdaFromValidator(queue.parsed.validator);
  const ix = ensureTransferQueueCrankIx(
    payer.publicKey,
    queue.pubkey,
    magicFeeVault,
    MAGIC_CONTEXT_ID,
    MAGIC_PROGRAM_ID,
  );

  const latestBlockhash = await connection.getLatestBlockhash("confirmed");
  const tx = new Transaction({
    feePayer: payer.publicKey,
    blockhash: latestBlockhash.blockhash,
    lastValidBlockHeight: latestBlockhash.lastValidBlockHeight,
  }).add(ix);
  tx.sign(payer);

  const signature = await connection.sendRawTransaction(tx.serialize(), { skipPreflight: false });
  await connection.confirmTransaction({ signature, ...latestBlockhash }, "confirmed");
  return signature;
}

async function tickQueues(connection, payerPath, mint, options) {
  section("tick", ANSI_MAGENTA);
  keyValue("payer path", cyan(payerPath), ANSI_MAGENTA);

  const payer = readKeypair(payerPath);
  keyValue("payer", cyan(payer.publicKey.toBase58()), ANSI_MAGENTA);
  if (options.verbose) {
    keyValue("RPC", cyan(connection.rpcEndpoint), ANSI_MAGENTA);
    keyValue("mint", cyan(mint.toBase58()), ANSI_MAGENTA);
  }

  const queues = await findQueues(connection, mint);
  if (queues.length === 0) {
    console.log(yellow("No matching queue accounts found."));
    return;
  }

  for (let i = 0; i < queues.length; i += 1) {
    const queue = queues[i];
    subsection(`queue ${i + 1}`, ANSI_CYAN);
    keyValue("address", cyan(queue.pubkey.toBase58()));
    if (options.verbose) {
      keyValue("length/capacity", colorLength(queue.parsed), queue.parsed.length > 0 ? ANSI_YELLOW : ANSI_GREEN);
      keyValue("validator", cyan(queue.parsed.validator.toBase58()));
    }

    const signature = await sendTick(connection, payer, queue);
    keyValue("signature", green(signature), ANSI_GREEN);
  }
}

async function ensureCranks(connection, payerPath, mint, options) {
  section("crank ensure", ANSI_MAGENTA);
  keyValue("payer path", cyan(payerPath), ANSI_MAGENTA);

  const payer = readKeypair(payerPath);
  keyValue("payer", cyan(payer.publicKey.toBase58()), ANSI_MAGENTA);
  if (options.verbose) {
    keyValue("RPC", cyan(connection.rpcEndpoint), ANSI_MAGENTA);
    keyValue("mint", cyan(mint.toBase58()), ANSI_MAGENTA);
    keyValue("instruction", cyan("EnsureTransferQueueCrank (17)"), ANSI_MAGENTA);
  }

  const queues = await findQueues(connection, mint);
  if (queues.length === 0) {
    console.log(yellow("No matching queue accounts found."));
    return;
  }

  for (let i = 0; i < queues.length; i += 1) {
    const queue = queues[i];
    const magicFeeVault = magicFeeVaultPdaFromValidator(queue.parsed.validator);
    subsection(`queue ${i + 1}`, ANSI_CYAN);
    keyValue("address", cyan(queue.pubkey.toBase58()));
    if (options.verbose) {
      keyValue("length/capacity", colorLength(queue.parsed), queue.parsed.length > 0 ? ANSI_YELLOW : ANSI_GREEN);
      keyValue("validator", cyan(queue.parsed.validator.toBase58()));
      keyValue("magic fee vault", cyan(magicFeeVault.toBase58()));
      keyValue("crank task id", queue.parsed.crankTaskId === 0n ? dim("none") : yellow(queue.parsed.crankTaskId.toString()));
    }

    const signature = await sendEnsureCrank(connection, payer, queue);
    keyValue("signature", green(signature), ANSI_GREEN);
  }
}

async function main() {
  const argv = process.argv.slice(2);
  const args = parseArgs(argv);
  const command = parseCommand(argv);

  if (argv.length === 0) {
    rootUsage();
    return;
  }

  if (command === "help") {
    printHelp(parseHelpTarget(argv));
    return;
  }

  if (args.help) {
    printHelp(command);
    return;
  }

  if (command !== "queue" && command !== "tick" && command !== "crank") {
    rootUsage();
    throw new Error(`Unknown command: ${command ?? "(none)"}`);
  }

  validateOptions(command, args);
  const options = { verbose: Boolean(args.verbose || args.full) };
  const erConnection = new Connection(DEFAULT_ER_RPC, "confirmed");

  if (command === "tick") {
    const payerPath = args.payer ?? process.env.PAYER_KEYPAIR ?? DEFAULT_PAYER_PATH;
    await tickQueues(erConnection, payerPath, DEFAULT_QUEUE_MINT, options);
    return;
  }

  if (command === "crank") {
    if (!args.ensure) {
      throw new Error("Use ./inspect crank --ensure");
    }
    const payerPath = args.payer ?? process.env.PAYER_KEYPAIR ?? DEFAULT_PAYER_PATH;
    await ensureCranks(erConnection, payerPath, DEFAULT_QUEUE_MINT, options);
    return;
  }

  const baseConnection = new Connection(DEFAULT_BASE_RPC, "confirmed");
  await scanQueues(erConnection, baseConnection, "er", DEFAULT_QUEUE_MINT, options);
}

main().catch((error) => {
  console.error("");
  console.error(`inspect failed: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
});
