use ephemeral_rollups_pinocchio::acl::PERMISSION_PROGRAM_ID;
use solana_account::Account;
use solana_program::{bpf_loader, rent::Rent};
use solana_program_test::{processor, read_file, ProgramTest};

use crate::utils::{associated_token_program_id, process_associated_token_program_mock};

fn add_associated_token_program(pt: &mut ProgramTest) {
    pt.prefer_bpf(false);
    pt.add_program(
        "spl_associated_token_account",
        associated_token_program_id(),
        processor!(process_associated_token_program_mock),
    );
    pt.prefer_bpf(true);
}

pub fn setup_program_test() -> ProgramTest {
    let mut pt = ProgramTest::new(
        "ephemeral_token_program",
        ephemeral_spl_api::program::ID,
        None,
    );

    // Add the associated token program
    add_associated_token_program(&mut pt);

    // Add the permission program
    let data = read_file("tests/fixtures/acl.so");
    pt.add_account(
        PERMISSION_PROGRAM_ID,
        Account {
            lamports: Rent::default().minimum_balance(data.len()).max(1),
            data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );

    // Setup the delegation program
    let data = read_file("tests/fixtures/dlp.so");
    pt.add_account(
        ephemeral_rollups_pinocchio::ID,
        Account {
            lamports: Rent::default().minimum_balance(data.len()).max(1),
            data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );

    pt.prefer_bpf(true);
    pt
}
