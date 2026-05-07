import {
  AddressLookupTableProgram,
  Connection,
  PublicKey,
} from "@solana/web3.js";

export type AddressLookupTable = NonNullable<Awaited<ReturnType<Connection["getAddressLookupTable"]>>["value"]>;

const connectionCache = new Map<string, Connection>();
const lookupTableCache = new Map<string, Promise<AddressLookupTable>>();
const lookupTableOwnerValidationCache = new Map<string, Promise<void>>();

type LookupTableCacheOptions = {
  validateOwner?: boolean;
};

export function getConnection(endpoint: string) {
  let connection = connectionCache.get(endpoint);

  if (!connection) {
    connection = new Connection(endpoint, "confirmed");
    connectionCache.set(endpoint, connection);
  }

  return connection;
}

export async function getCachedAddressLookupTable(
  endpoint: string,
  lookupTableAddress: PublicKey,
  options: LookupTableCacheOptions = {},
) {
  const cacheKey = `${endpoint}:${lookupTableAddress.toBase58()}`;
  let request = lookupTableCache.get(cacheKey);

  if (!request) {
    const connection = getConnection(endpoint);
    request = (async () => {
      const lookupTableResponse = await connection.getAddressLookupTable(lookupTableAddress);
      const lookupTable = lookupTableResponse.value;

      if (!lookupTable) {
        throw new Error("lookup table account was not found");
      }

      return lookupTable;
    })().catch((error) => {
      if (lookupTableCache.get(cacheKey) === request) {
        lookupTableCache.delete(cacheKey);
      }

      throw error;
    });

    lookupTableCache.set(cacheKey, request);
  }

  const lookupTable = await request;

  if (options.validateOwner) {
    await validateCachedAddressLookupTableOwner(endpoint, lookupTableAddress);
  }

  return lookupTable;
}

async function validateCachedAddressLookupTableOwner(
  endpoint: string,
  lookupTableAddress: PublicKey,
) {
  const cacheKey = `${endpoint}:${lookupTableAddress.toBase58()}`;
  let request = lookupTableOwnerValidationCache.get(cacheKey);

  if (!request) {
    const connection = getConnection(endpoint);
    request = (async () => {
      const lookupTableAccountInfo = await connection.getAccountInfo(lookupTableAddress, "confirmed");

      if (!lookupTableAccountInfo) {
        throw new Error("lookup table account info was not found");
      }

      if (!lookupTableAccountInfo.owner.equals(AddressLookupTableProgram.programId)) {
        throw new Error("lookup table account has unexpected owner");
      }
    })().catch((error) => {
      if (lookupTableOwnerValidationCache.get(cacheKey) === request) {
        lookupTableOwnerValidationCache.delete(cacheKey);
      }

      throw error;
    });

    lookupTableOwnerValidationCache.set(cacheKey, request);
  }

  return request;
}

export function getCachedAddressLookupTables(
  endpoint: string,
  lookupTableAddresses: PublicKey[],
  options: LookupTableCacheOptions = {},
) {
  return Promise.all(
    lookupTableAddresses.map((lookupTableAddress) =>
      getCachedAddressLookupTable(endpoint, lookupTableAddress, options)
    ),
  );
}
