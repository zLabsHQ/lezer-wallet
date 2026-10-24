//! `lezer_registry` — on-chain program directory for LP-0023.
//!
//! Registration is only possible through a *chained call*: the registered program's own plan
//! emits `ChainedCall { program_account_id: <this registry>, .. }`, so `caller_account_id`
//! authenticates the program — no signer input can forge it.

use borsh::BorshDeserialize as _;
use lee_core::{
    account::AccountId,
    program::{ApplyInput, PdaSeed, Plan, ProgramCall, apply_write, read_program_call},
};
use registry_core::{
    Effect, Entry, IndexPage, Instruction, MAX_ENTRIES_PER_PAGE, bucket_of, entry_seed, validate_metadata,
};

fn main() {
    match read_program_call::<Instruction>() {
        ProgramCall::Plan(input, instruction) => plan(&input, instruction).write(),
        ProgramCall::Apply(input) => apply(input),
    }
}

fn plan(input: &lee_core::program::PlanInput, instruction: Instruction) -> Plan {
    let mut plan = Plan::new(input);
    match instruction {
        Instruction::Register { name, source_url, commit, version } => {
            validate_metadata(&name, &source_url, &commit, &version).unwrap_or_else(|e| panic!("{e}"));
            // Chained calls only: the caller IS the program being registered.
            let program_id = input
                .caller_account_id
                .expect("Register must be invoked as a chained call from the program itself");
            let program_bytes = *program_id.value();

            let [entry, index_page] = input.accounts.as_slice() else {
                panic!("Register expects [entry, index_page]");
            };
            let expected_entry = pda(input, entry_seed(&program_bytes));
            assert_eq!(entry.account_id, expected_entry, "entry is not the registry PDA for this program");

            let _ = bucket_of(&program_bytes); // bucket affects which page the caller selects
            plan.effect(entry, &Effect::WriteEntry(Entry {
                program_id: program_bytes, name, source_url, commit, version,
            }));
            plan.effect(index_page, &Effect::AppendToIndex { program_id: program_bytes });
        }
    }
    plan
}

fn pda(input: &lee_core::program::PlanInput, seed: [u8; 32]) -> AccountId {
    AccountId::for_public_pda(&input.self_account_id, &PdaSeed::new(seed))
}

fn apply(input: ApplyInput) -> ! {
    let effect = Effect::try_from_slice(&input.effect_data).expect("a program only applies effects it planned");
    let post: Vec<u8> = match effect {
        Effect::WriteEntry(entry) => {
            // Re-registration is an in-place update: the entry account is unique per program.
            borsh::to_vec(&entry).expect("borsh serialization is infallible")
        }
        Effect::AppendToIndex { program_id } => {
            let mut page = if input.pre_data.is_empty() {
                IndexPage::default()
            } else {
                IndexPage::try_from_slice(&input.pre_data).expect("corrupt index page")
            };
            if !page.entries.contains(&program_id) {
                assert!(page.entries.len() < MAX_ENTRIES_PER_PAGE, "index page is full — use the next page");
                page.entries.push(program_id);
            }
            borsh::to_vec(&page).expect("borsh serialization is infallible")
        }
    };
    apply_write(input, post.try_into().expect("shard data fits the limit"))
}
