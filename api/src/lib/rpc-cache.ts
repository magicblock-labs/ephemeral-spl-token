import {
  AddressLookupTableProgram,
  Connection,
  PublicKey,
} from "@solana/web3.js";

export type AddressLookupTable = NonNullable<Awaited<ReturnType<Connection["getAddressLookupTable"]>>["value"]>;
export const ADDRESS_LOOKUP_TABLE_CACHE_TTL_MS = 3 * 60 * 60 * 1000;

const connectionCache = new Map<string, Connection>();
const lookupTableCache = new Map<string, CacheEntry<AddressLookupTable>>();
const lookupTableOwnerValidationCache = new Map<string, CacheEntry<void>>();

type CacheEntry<T> = {
  expiresAt: number;
  request: Promise<T>;
};

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
  const now = Date.now();
  let entry = lookupTableCache.get(cacheKey);

  if (!entry || entry.expiresAt <= now) {
    const connection = getConnection(endpoint);
    const request = (async () => {
      const lookupTableResponse = await connection.getAddressLookupTable(lookupTableAddress);
      const lookupTable = lookupTableResponse.value;

      if (!lookupTable) {
        throw new Error("lookup table account was not found");
      }

      return lookupTable;
    })().catch((error) => {
      if (lookupTableCache.get(cacheKey)?.request === request) {
        lookupTableCache.delete(cacheKey);
      }

      throw error;
    });

    entry = {
      expiresAt: now + ADDRESS_LOOKUP_TABLE_CACHE_TTL_MS,
      request,
    };
    lookupTableCache.set(cacheKey, entry);
  }

  const lookupTable = await entry.request;

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
  const now = Date.now();
  let entry = lookupTableOwnerValidationCache.get(cacheKey);

  if (!entry || entry.expiresAt <= now) {
    const connection = getConnection(endpoint);
    const request = (async () => {
      const lookupTableAccountInfo = await connection.getAccountInfo(lookupTableAddress, "confirmed");

      if (!lookupTableAccountInfo) {
        throw new Error("lookup table account info was not found");
      }

      if (!lookupTableAccountInfo.owner.equals(AddressLookupTableProgram.programId)) {
        throw new Error("lookup table account has unexpected owner");
      }
    })().catch((error) => {
      if (lookupTableOwnerValidationCache.get(cacheKey)?.request === request) {
        lookupTableOwnerValidationCache.delete(cacheKey);
      }

      throw error;
    });

    entry = {
      expiresAt: now + ADDRESS_LOOKUP_TABLE_CACHE_TTL_MS,
      request,
    };
    lookupTableOwnerValidationCache.set(cacheKey, entry);
  }

  return entry.request;
}

export function getCachedAddressLookupTables(
  endpoint: string,
  lookupTableAddresses: PublicKey[],
  options: LookupTableCacheOptions = {},
) {
  return Promise.all(
    lookupTableAddresses.map(lookupTableAddress =>
      getCachedAddressLookupTable(endpoint, lookupTableAddress, options),
    ),
  );
}
