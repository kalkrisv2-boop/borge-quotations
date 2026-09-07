//! Usage: cargo run --example hash_password -- "YourRealPasswordHere"
//!
//! Prints a PBKDF2-SHA512 hash in the exact "{salt_hex}${hash_hex}" format
//! `auth::verify_password` expects, for pasting into `.env` as
//! `SEED_USER_PASSWORD_HASH`. A fresh random salt is generated each run, so running
//! this twice with the same password produces two different (both valid) hashes.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let password = match args.get(1) {
        Some(p) if !p.is_empty() => p,
        _ => {
            eprintln!("Usage: cargo run --example hash_password -- \"YourRealPasswordHere\"");
            std::process::exit(1);
        }
    };

    let hash = borge_equipment_rental_lib::auth::hash_password(password, None);
    println!("{}", hash);
    eprintln!("\nPaste this into your .env as:\nSEED_USER_PASSWORD_HASH={}", hash);
}
