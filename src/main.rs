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
fn fmt_attrs(node: &whatsapp_rust::wacore_binary::NodeRef<'_>) -> String {
    let mut s = String::new();
    for (k, v) in node.attrs_iter() {
        s.push_str(&format!(" {}={}", k, v.as_str()));
    }
    s
}

/// Diagnostik: rekam atribut lengkap node masuk (<ack>, <iq><group>, <message>)
/// yang sering disamarkan oleh StanzaSummary standar.
fn dump_raw_node(node: &whatsapp_rust::wacore_binary::NodeRef<'_>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    const KIPAS: &str = "120363431048197201";
    const RR: &str = "120363404191362374";
    static MSG_DUMP_COUNT: AtomicUsize = AtomicUsize::new(0);

    match &*node.tag {
        "ack" => {
            let from = node
                .get_attr("from")
                .map(|v| v.as_str().into_owned())
                .unwrap_or_default();
            let has_err = node.get_attr("error").is_some();
            if has_err || from.starts_with(KIPAS) || from.starts_with(RR) {
                eprintln!("[RAW-ACK] attrs:{}", fmt_attrs(node));
                if let Some(children) = node.children() {
                    for c in children {
                        eprintln!("[RAW-ACK] child <{}> attrs:{}", c.tag, fmt_attrs(c));
                    }
                }
            }
        }
        "iq" => {
            if let Some(children) = node.children() {
                for c in children {
                    if &*c.tag == "group" {
                        let gid = c
                            .get_attr("id")
                            .map(|v| v.as_str().into_owned())
                            .unwrap_or_default();
                        if gid.starts_with(KIPAS) || gid.starts_with(RR) {
                            eprintln!("[RAW-GROUP] attrs:{}", fmt_attrs(c));
                            if let Some(gc) = c.children() {
                                for sub in gc {
                                    if &*sub.tag != "participant" {
                                        eprintln!(
                                            "[RAW-GROUP] child <{}> attrs:{} text={:?}",
                                            sub.tag,
                                            fmt_attrs(sub),
                                            sub.content_str()
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        "message" => {
            let from = node
                .get_attr("from")
                .map(|v| v.as_str().into_owned())
                .unwrap_or_default();
            if from.starts_with(KIPAS) && MSG_DUMP_COUNT.fetch_add(1, Ordering::Relaxed) < 12 {
                eprintln!("[RAW-MSG] attrs:{}", fmt_attrs(node));
                if let Some(children) = node.children() {
                    for c in children {
                        eprintln!("[RAW-MSG] child <{}> attrs:{}", c.tag, fmt_attrs(c));
                    }
                }
            }
        }
        _ => {}
    }
}

/// Diagnostik: dump stanza `<message>` keluar untuk grup yang bermasalah.
/// Membantu melihat `addressing_mode`, `phash`, dan isi `<participants>`
/// (apakah ada JID PN yang bocor ke grup LID, dsb).
fn dump_sent_group_frame(plaintext: &[u8]) {
    use std::sync::atomic::{AtomicUsize, Ordering};

    const KIPAS: &str = "120363431048197201";
    const TABUNG: &str = "120363309940031565";
    const RR: &str = "120363404191362374";
    static DUMP_COUNT: AtomicUsize = AtomicUsize::new(0);

    let Ok(node) = whatsapp_rust::wacore_binary::marshal::unmarshal_packed_ref(plaintext) else {
        return;
    };
    if node.tag != "message" {
        return;
    }
    let Some(to) = node.get_attr("to") else {
        return;
    };
    let to_str = to.as_str();
    let target = if to_str.starts_with(KIPAS) {
        KIPAS
    } else if to_str.starts_with(TABUNG) {
        TABUNG
    } else if to_str.starts_with(RR) {
        RR
    } else {
        return;
    };
    // Batasi supaya log tidak banjir.
    if DUMP_COUNT.fetch_add(1, Ordering::Relaxed) >= 20 {
        return;
    }

    let mut attrs = String::new();
    for (k, v) in node.attrs_iter() {
        attrs.push_str(&format!(" {}={}", k, v.as_str()));
    }
    eprintln!("[SENT-FRAME] to={to_str} attrs:{attrs}");

    let mut saw_participants = false;
    if let Some(children) = node.children() {
        for child in children {
            match &*child.tag {
                "participants" => {
                    saw_participants = true;
                    let tos = child.children().unwrap_or(&[]);
                    let mut lid = 0usize;
                    let mut pn = 0usize;
                    let mut other = 0usize;
                    let mut pn_list: Vec<String> = Vec::new();
                    let mut other_list: Vec<String> = Vec::new();
                    for t in tos {
                        let jid = t
                            .get_attr("jid")
                            .map(|v| v.as_str().into_owned())
                            .unwrap_or_default();
                        if jid.ends_with("@lid") {
                            lid += 1;
                        } else if jid.ends_with("@s.whatsapp.net") {
                            pn += 1;
                            if pn_list.len() < 30 {
                                pn_list.push(jid);
                            }
                        } else {
                            other += 1;
                            if other_list.len() < 30 {
                                other_list.push(jid);
                            }
                        }
                    }
                    eprintln!(
                        "[SENT-FRAME] group={target} participants total={} lid={} pn={} other={}",
                        tos.len(),
                        lid,
                        pn,
                        other
                    );
                    for jid in &pn_list {
                        eprintln!("[SENT-FRAME] PN-IN-LID: {jid}");
                    }
                    for jid in &other_list {
                        eprintln!("[SENT-FRAME] OTHER-SERVER: {jid}");
                    }
                    if target == KIPAS {
                        for t in tos {
                            let jid = t
                                .get_attr("jid")
                                .map(|v| v.as_str().into_owned())
                                .unwrap_or_default();
                            eprintln!("[SENT-FRAME] to {jid}");
                        }
                    }
                }
                "enc" => {
                    let etype = child
                        .get_attr("type")
                        .map(|v| v.as_str().into_owned())
                        .unwrap_or_default();
                    eprintln!(
                        "[SENT-FRAME] enc type={etype} bytes={}",
                        child.content_bytes().map(|b| b.len()).unwrap_or(0)
                    );
                }
                tag => {
                    eprintln!("[SENT-FRAME] child <{tag}>");
                }
            }
        }
    }
    if !saw_participants {
        eprintln!("[SENT-FRAME] group={target} NO-PARTICIPANTS-NODE");
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
    // Diagnostik: tampilkan warning/error internal whatsapp-rust (mis. sender key hilang).
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
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
    let store = SqliteStore::open(&db_str).await?;

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
        .on_connected(|client| async move {
            eprintln!("\n[+] Berhasil terhubung ke WhatsApp!");
            // Diagnostik: aktifkan event kegagalan dekripsi (butuh lease selama proses hidup).
            std::mem::forget(client.acquire_enc_decrypt_failed_forwarding());
            // Diagnostik: tangkap stanza keluar untuk membedah framing grup.
            std::mem::forget(client.acquire_sent_frame_forwarding());
            // Diagnostik: tangkap node masuk mentah (<ack>, <iq><group>, <message>).
            std::mem::forget(client.acquire_raw_node_forwarding());

            // Diagnostik: daftar semua grup yang diikuti akun bot.
            let c = Arc::clone(&client);
            tokio::spawn(async move {
                match c.groups().list_participating().await {
                    Ok(list) => {
                        eprintln!("[GROUPS] total={}", list.len());
                        for g in &list {
                            eprintln!(
                                "[GROUPS] {} | {} | members={:?} | hierarchy={:?}",
                                g.id,
                                g.subject.as_deref().unwrap_or("(tanpa nama)"),
                                g.participant_count,
                                g.hierarchy
                            );
                        }
                    }
                    Err(e) => eprintln!("[GROUPS] gagal ambil daftar grup: {e}"),
                }
            });
        })
        .on_event_for(
            &[
                EventKind::EncDecryptFailed,
                EventKind::UndecryptableMessage,
                EventKind::GroupUpdate,
                EventKind::SentFrame,
                EventKind::RawNode,
            ],
            |event, _client| async move {
                match &*event {
                    Event::EncDecryptFailed(e) => {
                        eprintln!(
                            "[DECRYPT-FAIL] chat={} sender={} id={} enc={:?} reason={:?}",
                            e.info.source.chat,
                            e.info.source.sender,
                            e.info.id,
                            e.enc_type,
                            e.reason
                        );
                    }
                    Event::UndecryptableMessage(e) => {
                        eprintln!(
                            "[UNDECRYPTABLE] chat={} sender={} id={} unavailable={} utype={:?}",
                            e.info.source.chat,
                            e.info.source.sender,
                            e.info.id,
                            e.is_unavailable,
                            e.unavailable_type
                        );
                    }
                    Event::GroupUpdate(e) => {
                        eprintln!(
                            "[GROUP-UPDATE] group={} actor={:?} lid_mode={} action={:?}",
                            e.group_jid, e.participant, e.is_lid_addressing_mode, e.action
                        );
                    }
                    Event::SentFrame(e) => {
                        dump_sent_group_frame(&e.plaintext);
                    }
                    Event::RawNode(node) => {
                        dump_raw_node(node.get());
                    }
                    _ => {}
                }
            },
        )
        .on_logged_out(|info| async move {
            eprintln!("[-] Sesi logout / dihentikan: reason={:?}, on_connect={}", info.reason, info.on_connect);
            eprintln!("[-] Sesi WhatsApp di database sudah dicabut/di-logout.");
            eprintln!("[-] Membutuhkan pairing ulang. Menunggu 15 detik sebelum keluar...");
            tokio::time::sleep(Duration::from_secs(15)).await;
        })
        .on_message(move |ctx| {
            let reg = reg.clone();
            async move {
                let text = commands::extract_message_text(&ctx.message);
                // Simpan pesan view-once ke cache per-chat agar bisa dibuka dengan /rvo
                if ctx.message.is_view_once() {
                    let chat_id = ctx.info.source.chat.to_string();
                    reg.bot_ctx.store_view_once(chat_id, Arc::clone(&ctx.message));
                }

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
