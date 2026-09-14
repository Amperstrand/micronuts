//! M1 scaffold entry: NVS up, the ProofStore live, a diagnostic console.

use esp_idf_svc::nvs::EspDefaultNvsPartition;

use cashu_core_lite::store::ProofStore as _;
use micronuts_esp32_wallet::{NvsProofStore, WALLET_KEY};

fn main() -> anyhow::Result<()> {
    esp_idf_sys::link_patches();

    // 32 KiB fail-stop bound: wallet blobs that cannot fit NVS atomically
    // fail loudly here — never the nucula#1 silent RAM-only class.
    let partition = EspDefaultNvsPartition::take()?;
    let mut store = NvsProofStore::new(partition, WALLET_KEY, 32 * 1024).map_err(|e| anyhow::anyhow!("store: {e:?}"))?;

    println!("micronuts-esp32-wallet M1 scaffold");
    println!("stored blob: {} B", store.stored_len().map_err(|e| anyhow::anyhow!("{e:?}"))?);

    // Round-trip smoke through the full ProofStore contract.
    let payload: Vec<u8> = (0..64u8).collect();
    store.save(&payload).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let back = store.load().map_err(|e| anyhow::anyhow!("{e:?}"))?;
    assert_eq!(back.as_deref(), Some(payload.as_slice()));
    println!("store round-trip: OK (64 B)");

    let mut line = String::new();
    println!("type 'help' for commands");
    loop {
        line.clear();
        std::io::stdin().read_line(&mut line)?;
        match line.trim() {
            "help" => println!("help | status | heap"),
            "status" => println!(
                "stored: {} B | bound: 32768 B | core: cashu-core-lite",
                store.stored_len().map_err(|e| anyhow::anyhow!("{e:?}"))?
            ),
            "heap" => println!("free: {} B", unsafe { esp_idf_svc::hal::sys::esp_get_free_heap_size() }),
            "" => {}
            other => println!("unknown: {other}"),
        }
    }
}
