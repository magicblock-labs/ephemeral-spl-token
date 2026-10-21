# Program upgrades

Upgrades go through a Squads multisig whose vault holds the upgrade authority.
CI builds the program and proposes an upgrade; multisig members approve and
execute it.

## 1. Build and propose

Run **Actions → Propose Program Upgrade** with the target `cluster` and
`dry_run` enabled. This builds, uploads, and simulates the upgrade, then closes
the buffer without opening a proposal.

After reviewing the dry run, run the workflow with `dry_run` disabled to open a
Squads proposal that upgrades the program and records its verified build.

## 2. Approve and execute

Multisig members approve and execute the proposal through the
[Squads app](https://app.squads.so) on mainnet, or the
[shared command-line tools](https://github.com/magicblock-labs/.github/tree/main/actions/squads-program-upgrade#approving-and-managing-the-multisig-devnet)
on devnet.

## 3. Request remote verification

After each mainnet upgrade executes, run **Actions → Verify Program (Remote)**
to request remote verification for explorers.

## Workflow configuration

The [proposal workflow](../.github/workflows/propose-program-upgrade.yml) defines
cluster environments, credentials, the program ID, and the upgrade build's
Solana version. The [remote verification workflow](../.github/workflows/verify-program-remote.yml)
requests verification separately after execution.
