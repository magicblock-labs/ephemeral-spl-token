import { afterEach, describe, expect, it, vi } from "vitest";
import {
  AddressLookupTableAccount,
  AddressLookupTableProgram,
  type AccountInfo,
  Connection,
  Keypair,
  PublicKey,
} from "@solana/web3.js";

import {
  ADDRESS_LOOKUP_TABLE_CACHE_TTL_MS,
  getCachedAddressLookupTable,
} from "./rpc-cache";

function createLookupTableResponse(value: AddressLookupTableAccount | null): Awaited<ReturnType<Connection["getAddressLookupTable"]>> {
  return {
    context: {
      slot: 0,
    },
    value,
  };
}

function createLookupTableAccount(addresses: PublicKey[], key = Keypair.generate().publicKey) {
  return new AddressLookupTableAccount({
    key,
    state: {
      deactivationSlot: 18446744073709551615n,
      lastExtendedSlot: 0,
      lastExtendedSlotStartIndex: 0,
      authority: Keypair.generate().publicKey,
      addresses,
    },
  });
}

function createLookupTableAccountInfo(): AccountInfo<Buffer> {
  return {
    data: Buffer.alloc(0),
    executable: false,
    lamports: 0,
    owner: AddressLookupTableProgram.programId,
    rentEpoch: 0,
  };
}

describe("rpc cache", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("refreshes cached address lookup tables after the TTL expires", async () => {
    let now = 1_000;
    vi.spyOn(Date, "now").mockImplementation(() => now);

    const endpoint = "https://base.lookup-table-ttl.rpc.test";
    const lookupTableAddress = Keypair.generate().publicKey;
    const lookupTable = createLookupTableAccount(
      [Keypair.generate().publicKey],
      lookupTableAddress,
    );
    const getAddressLookupTableSpy = vi
      .spyOn(Connection.prototype, "getAddressLookupTable")
      .mockResolvedValue(createLookupTableResponse(lookupTable));

    await getCachedAddressLookupTable(endpoint, lookupTableAddress);
    await getCachedAddressLookupTable(endpoint, lookupTableAddress);

    expect(getAddressLookupTableSpy).toHaveBeenCalledOnce();

    now += ADDRESS_LOOKUP_TABLE_CACHE_TTL_MS + 1;
    await getCachedAddressLookupTable(endpoint, lookupTableAddress);

    expect(getAddressLookupTableSpy).toHaveBeenCalledTimes(2);
  });

  it("refreshes cached address lookup table owner validation after the TTL expires", async () => {
    let now = 2_000;
    vi.spyOn(Date, "now").mockImplementation(() => now);

    const endpoint = "https://base.lookup-table-owner-ttl.rpc.test";
    const lookupTableAddress = Keypair.generate().publicKey;
    const lookupTable = createLookupTableAccount(
      [Keypair.generate().publicKey],
      lookupTableAddress,
    );
    vi.spyOn(Connection.prototype, "getAddressLookupTable")
      .mockResolvedValue(createLookupTableResponse(lookupTable));
    const getAccountInfoSpy = vi
      .spyOn(Connection.prototype, "getAccountInfo")
      .mockResolvedValue(createLookupTableAccountInfo());

    await getCachedAddressLookupTable(endpoint, lookupTableAddress, { validateOwner: true });
    await getCachedAddressLookupTable(endpoint, lookupTableAddress, { validateOwner: true });

    expect(getAccountInfoSpy).toHaveBeenCalledOnce();

    now += ADDRESS_LOOKUP_TABLE_CACHE_TTL_MS + 1;
    await getCachedAddressLookupTable(endpoint, lookupTableAddress, { validateOwner: true });

    expect(getAccountInfoSpy).toHaveBeenCalledTimes(2);
  });
});
