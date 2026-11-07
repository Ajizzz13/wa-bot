mod commands;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use commands::CommandRegistry;
use whatsapp_rust::pair_code::PairCodeOptions;
use whatsapp_rust::prelude::*;


pub fn bot_memory_stat() -> String {
    let mut rss_kb: Option<u64> = None;
    let mut anon_kb: Option<u64> = None;

    if let Ok(content) = std::fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if line.starts_with("VmRSS:") {
                rss_kb = line.split_whitespace().nth(1).and_then(|v| v.parse().ok());
            } else if line.starts_with("RssAnon:") {
                anon_kb = line.split_whitespace().nth(1).and_then(|v| v.parse().ok());
            }
        }
    }

    match (rss_kb, anon_kb) {
        (Some(rss), Some(anon)) => {
            let rss_mb = rss as f64 / 1024.0;
            let anon_mb = anon as f64 / 1024.0;
            format!("RSS: {rss_mb:.1} MB ({rss} kB) | Heap/Anon: {anon_mb:.1} MB")
        }
        (Some(rss), None) => {
            let rss_mb = rss as f64 / 1024.0;
            format!("RSS: {rss_mb:.1} MB ({rss} kB)")
        }
        _ => "RAM: N/A".to_string(),
    }
}

pub fn media_memory_checkpoint(stage: &str) {
    if !has_memory_flag() {
        return;
    }

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        let heap = libc::mallinfo2();
        eprintln!(
            "[RAM {stage}] {} | heap-live: {:.1} MB | arena-free: {:.1} MB | mmap-live: {:.1} MB",
            bot_memory_stat(),
            heap.uordblks as f64 / 1024.0 / 1024.0,
            heap.fordblks as f64 / 1024.0 / 1024.0,
            heap.hblkhd as f64 / 1024.0 / 1024.0,
        );
    }

    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    eprintln!("[RAM {stage}] {}", bot_memory_stat());
}

fn has_memory_flag() -> bool {
    std::env::args().any(|arg| arg == "-m" || arg == "--memory" || arg == "-mem")
}

fn session_path() -> PathBuf {
    std::env::var_os("WA_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("whatsapp.db"))
}

fn get_phone_number() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--phone" || arg == "-p" {
            if let Some(val) = args.next() {
                let cleaned: String = val.chars().filter(|c| c.is_ascii_digit()).collect();
                if !cleaned.is_empty() {
                    return Some(cleaned);
                }
            }
        }
    }
    if let Ok(val) = std::env::var("WA_PHONE") {
        let cleaned: String = val.chars().filter(|c| c.is_ascii_digit()).collect();
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    None
}

fn configure_memory_allocator() {
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 128 * 1024);
    }
}
async fn log_inbound_message(ctx: &whatsapp_rust::bot::MessageContext, text: &str) {
    let push_name = if ctx.info.push_name.trim().is_empty() {
        "Anonim".to_string()
    } else {
        ctx.info.push_name.trim().to_string()
    };

    let sender = &ctx.info.source.sender;
    let mut phone = sender.user_base().to_string();
    if let Ok(Some(entry)) = ctx.client.get_lid_pn_entry(sender).await {
        phone = entry.phone_number.to_string();
    }

    let is_group = commands::is_group_chat(&ctx.info.source.chat);
    let mem = bot_memory_stat();

    eprintln!();
    if is_group {
        eprintln!("┏━━━ [ 👥 GROUP INBOUND ] ━━━");
        eprintln!("┃ 👤 Pengirim : {push_name} ({phone})");
        eprintln!("┃ 🏷️ Grup     : {}", ctx.info.source.chat);
        eprintln!("┃ 📊 Metrik   : Group Chat • {mem}");
    } else {
        eprintln!("┏━━━ [ ✉️ INBOUND ] ━━━");
        eprintln!("┃ 👤 Pengirim : {push_name} ({phone})");
        eprintln!("┃ 📊 Metrik   : Private Chat • {mem}");
    }
    eprintln!("┃ 💬 Pesan    :");
    for line in text.lines() {
        eprintln!("┃   {line}");
    }
    eprintln!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    configure_memory_allocator();
    let db = session_path();
    eprintln!("session db: {}", db.display());

    let mem_monitor = has_memory_flag();
    if mem_monitor {
        eprintln!("[RAM Monitor] AKTIF: Logging usage RAM realtime (interval 5s & per pesan)");
        tokio::spawn(async {
            let mut ticker = tokio::time::interval(Duration::from_secs(5));
            loop {
                ticker.tick().await;
                eprintln!("[RAM Ticker] {}", bot_memory_stat());
            }
        });
    }

    let db_str = db.to_string_lossy();
    let store = SqliteStore::new(&db_str).await?;

    let phone_opt = get_phone_number();
    let registry = Arc::new(CommandRegistry::new(phone_opt.clone()));
    let reg = registry.clone();

    let mut builder = Bot::builder()
        .with_backend(store)
        .skip_history_sync()
        .on_pair_code(|code, timeout| async move {
            eprintln!("\n========================================");
            eprintln!(" WHATSAPP PAIRING CODE:  {}", code);
            eprintln!(" Berlaku: {} detik", timeout.as_secs());
            eprintln!(" Masukkan kode ini di WhatsApp HP:");
            eprintln!(" Perangkat Tertaut > Tautkan dengan nomor telepon");
            eprintln!("========================================\n");
        })
        .on_pair_code_error(|err, _| async move {
            eprintln!("Pair code error: {:?}", err);
        })
        .on_connected(|_client| async {
            eprintln!("\n[+] Berhasil terhubung ke WhatsApp!");
        })
        .on_logged_out(|_info| async {
            eprintln!("[-] Sesi logout / dihentikan");
        })
        .on_message(move |ctx| {
            let reg = reg.clone();
            async move {
                let text = commands::extract_message_text(&ctx.message);
                // Jika pesan dari bot sendiri (is_from_me), hanya proses jika berupa command berawalan '/'
                if ctx.info.source.is_from_me {
                    if let Some(t) = text {
                        if !t.starts_with('/') {
                            return;
                        }
                    } else {
                        return;
                    }
                }

                // Cetak log vertikal berstruktur jika pesan memiliki teks
                if let Some(t) = text {
                    log_inbound_message(&ctx, t).await;
                }

                let handled = reg.dispatch(&ctx).await;
                if handled && mem_monitor {
                    eprintln!("[RAM Setelah Command] {}", bot_memory_stat());
                }
            }
        });

    if let Some(phone) = phone_opt {
        eprintln!("Menggunakan nomor pairing: {phone}");
        builder = builder.with_pair_code(PairCodeOptions {
            phone_number: phone,
            show_push_notification: true,
            ..Default::default()
        });
    } else {
        eprintln!("Catatan: Tanpa --phone/WA_PHONE. Menggunakan session database yang ada.");
    }

    let bot = builder.build().await?;
    let mut bot = bot.spawn();

    tokio::select! {
        reason = &mut bot => {
            eprintln!("session ended: {reason:?}");
        }
        _ = tokio::signal::ctrl_c() => {
            eprintln!("ctrl-c, shutting down");
            tokio::time::timeout(Duration::from_secs(5), bot.shutdown())
                .await
                .ok();
        }
    }
    Ok(())
}
