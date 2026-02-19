import {
  Connection,
  Transaction,
  Keypair,
} from "@solana/web3.js";
import * as fs from "fs";
import * as path from "path";
import * as os from "os";
import axios from "axios";
import {
  getAuthToken,
} from "@magicblock-labs/ephemeral-rollups-sdk";
import nacl from "tweetnacl";
import { describe, it, expect } from "vitest";
import dotenv from "dotenv";

dotenv.config();

describe("Ephemeral SPL Token Transactions", async () => {
  const TEST_TIMEOUT = 120_000;
  console.log("Ephemeral SPL Token Transaction Tests");

  // Configuration
  const ENDPOINT_RPC = process.env.PROVIDER_ENDPOINT || "https://api.devnet.solana.com";
  const API_URL = process.env.API_URL || "https://api.docs.magicblock.app";
  const EPHEMERAL_ENDPOINT_RPC = process.env.EPHEMERAL_ENDPOINT_RPC || "https://tee.magicblock.app";
  const MINT = process.env.MINT || "BTGw9VWza6Cr1NowmKAnNc8y1UTS4Vs9DkjzSnVkAEaz";
  console.log("Using RPC Endpoint:", ENDPOINT_RPC);
  console.log("Using API URL:", API_URL);
  console.log("Using Ephemeral Endpoint URL:", EPHEMERAL_ENDPOINT_RPC);
  console.log("Using Mint:", MINT);

  // Connections
  const connection = new Connection(ENDPOINT_RPC, "confirmed");
  console.log("Base Layer Connection:", connection.rpcEndpoint);

  // Load keypair
  const keypairPath = path.join(os.homedir(), ".config/solana/id.json");
  const secretKey = JSON.parse(fs.readFileSync(keypairPath, "utf-8"));
  const keypair = Keypair.fromSecretKey(Buffer.from(secretKey));

  console.log("Wallet:", keypair.publicKey.toString());

  interface TransactionResponse {
    transaction: string;
    message?: string;
  }

  // Helper function to deserialize and log transaction
  function deserializeAndLogTransaction(
    txBase64: string,
    testName: string
  ): Transaction {
    const txBuffer = Buffer.from(txBase64, "base64");
    const tx = Transaction.from(txBuffer);

    console.log(`\n=== ${testName} - Account Meta ===`);
    tx.instructions.forEach((instruction, index) => {
      console.log(`\nInstruction ${index}:`);
      console.log("  Program ID:", instruction.programId.toString());
      console.log("  Account Meta:");
      instruction.keys.forEach((key, keyIndex) => {
        console.log(`    [${keyIndex}] ${key.pubkey.toString()}`);
        console.log(`        isSigner: ${key.isSigner}, isWritable: ${key.isWritable}`);
      });
      console.log("  Data (hex):", instruction.data.toString("hex"));
    });
    console.log("=====================================\n");

    return tx;
  }

  // Helper function to send and confirm transaction
  async function sendAndConfirmTx(
    conn: Connection,
    tx: Transaction
  ): Promise<string> {
    const { blockhash, lastValidBlockHeight } =
      await conn.getLatestBlockhash("confirmed");

    tx.recentBlockhash = blockhash;
    tx.lastValidBlockHeight = lastValidBlockHeight;
    tx.sign(keypair);

    const serializedTx = tx.serialize();
    const signature = await conn.sendRawTransaction(serializedTx, {
      skipPreflight: true,
    });

    console.log("✓ Transaction Signature:", signature);

    const confirmation = await conn.confirmTransaction(signature);
    const isConfirmed = confirmation.value.err === null;
    
    if (!isConfirmed) {
      console.error("Transaction error:", confirmation.value.err);
      throw new Error(`Transaction failed: ${JSON.stringify(confirmation.value.err)}`);
    }

    return signature;
  }

  it(
    "Deposit SPL tokens to ephemeral ATA",
    async () => {
      const start = Date.now();
      const depositPayload = {
        payer: keypair.publicKey.toString(),
        user: keypair.publicKey.toString(),
        mint: MINT,
        amount: 10,
        endpoint_url: ENDPOINT_RPC,
      };
      try {
        const response = await axios.post(
          `${API_URL}/private/tx/deposit`,
          depositPayload,
          {
            headers: {
              accept: "application/json",
              "Content-Type": "application/json",
            },
          }
        );

        const data = response.data as TransactionResponse;
        const tx = deserializeAndLogTransaction(data.transaction, "Deposit");

        const signature = await sendAndConfirmTx(connection, tx);

        const duration = Date.now() - start;
        console.log(`${duration}ms - Deposit transaction completed`);
        expect(signature).toBeDefined();
      } catch (error: any) {
        console.error("Error response:", error.response?.data);
        throw error;
      }
    },
    TEST_TIMEOUT
  );

  it(
    "Prepare withdrawal from ephemeral ATA",
    async () => {
      const start = Date.now();

      // Get auth token
      const authToken = await getAuthToken(
        EPHEMERAL_ENDPOINT_RPC,
        keypair.publicKey,
        (message: Uint8Array) =>
          Promise.resolve(nacl.sign.detached(message, keypair.secretKey))
      );

      // Add token as query parameter to endpoint URL
      const teeEndpointWithToken = `${EPHEMERAL_ENDPOINT_RPC}?token=${authToken.token}`;
      console.log("TEE Endpoint with Token:", teeEndpointWithToken);

      const prepareWithdrawalPayload = {
        user: keypair.publicKey.toString(),
        mint: MINT,
        endpoint_url: teeEndpointWithToken,
      };

      const response = await axios.post(
        `${API_URL}/private/tx/prepare-withdrawal`,
        prepareWithdrawalPayload,
        {
          headers: {
            accept: "application/json",
            "Content-Type": "application/json",
          },
        }
      );

      const data = response.data as TransactionResponse;
      const tx = deserializeAndLogTransaction(data.transaction, "Prepare Withdrawal");

      const teeConnection = new Connection(teeEndpointWithToken, "confirmed");
      const signature = await sendAndConfirmTx(teeConnection, tx);

      const duration = Date.now() - start;
      console.log(`${duration}ms - Prepare withdrawal transaction completed`);
      expect(signature).toBeDefined();
    },
    TEST_TIMEOUT
  );

  it(
    "Withdraw SPL tokens from ephemeral ATA",
    async () => {
      const start = Date.now();

      const withdrawalPayload = {
        owner: keypair.publicKey.toString(),
        user: keypair.publicKey.toString(),
        mint: MINT,
        amount: 1,
        endpoint_url: ENDPOINT_RPC,
      };

      const response = await axios.post(
        `${API_URL}/private/tx/withdraw`,
        withdrawalPayload,
        {
          headers: {
            accept: "application/json",
            "Content-Type": "application/json",
          },
        }
      );

      const data = response.data as TransactionResponse;
      const tx = deserializeAndLogTransaction(data.transaction, "Withdrawal");

      const signature = await sendAndConfirmTx(connection, tx);

      const duration = Date.now() - start;
      console.log(`${duration}ms - Withdrawal transaction completed`);
      expect(signature).toBeDefined();
    },
    TEST_TIMEOUT
  );
});
