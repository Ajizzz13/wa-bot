use std::str::FromStr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::Jid;

use super::{is_owner, BotContext, BoxFut, Command, CommandResult};

const SCHEDULE_FILE: &str = "online_schedules.json";
const MIN_INTERVAL_SECS: u64 = 3 * 60;
const MAX_INTERVAL_SECS: u64 = 24 * 60 * 60;

#[derive(Clone, Debug)]
pub struct OnlineSchedule {
    pub id: String,
    pub interval_secs: u64,
    pub next_run: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn load_schedules() -> Vec<OnlineSchedule> {
    let Ok(content) = std::fs::read_to_string(SCHEDULE_FILE) else {
        return Vec::new();
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    let Some(arr) = val.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            let id = v.get("id")?.as_str()?.to_string();
            let interval_secs = v.get("interval_secs")?.as_u64()?;
            let next_run = v.get("next_run").and_then(|x| x.as_u64()).unwrap_or_else(now_secs);
            Some(OnlineSchedule {
                id,
                interval_secs,
                next_run,
            })
        })
        .collect()
}

pub fn save_schedules(list: &[OnlineSchedule]) {
    let arr: Vec<serde_json::Value> = list
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "interval_secs": s.interval_secs,
                "next_run": s.next_run,
            })
        })
        .collect();
    if let Ok(j) = serde_json::to_string_pretty(&serde_json::Value::Array(arr)) {
        let _ = std::fs::write(SCHEDULE_FILE, j);
    }
}

fn valid_id(id: &str) -> bool {
    id.len() >= 8 && id.len() <= 20 && id.chars().all(|c| c.is_ascii_digit())
}

fn parse_interval(raw: &str) -> Option<u64> {
    let s = raw.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s.parse::<u64>().ok().map(|m| m.saturating_mul(60));
    }
    let mut total: u64 = 0;
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: u64 = num.parse().ok()?;
            num.clear();
            total = total.saturating_add(match c {
                's' => n,
                'm' => n.saturating_mul(60),
                'h' => n.saturating_mul(3600),
                'd' => n.saturating_mul(86400),
                _ => return None,
            });
        }
    }
    if !num.is_empty() {
        total = total.saturating_add(num.parse::<u64>().ok()?.saturating_mul(60));
    }
    if total == 0 {
        None
    } else {
        Some(total)
    }
}

fn fmt_interval(secs: u64) -> String {
    if secs % 86400 == 0 {
        format!("{}d", secs / 86400)
    } else if secs % 3600 == 0 {
        format!("{}h", secs / 3600)
    } else if secs % 60 == 0 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

fn field<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("-")
}

fn format_result(res: &freefire_online::OnlineResult, routine: bool, jadwal: Option<&str>) -> String {
    let online = res.online.get("online").and_then(|x| x.as_bool()).unwrap_or(false);
    let timeout = res.online.get("timeout").and_then(|x| x.as_bool()).unwrap_or(false);
    let (badge, status) = if timeout {
        ("❔", "UNKNOWN")
    } else if online {
        ("🟢", "ONLINE")
    } else {
        ("🔴", "OFFLINE")
    };
    let mode = field(&res.online, "modeName");
    let map = field(&res.online, "mapName");
    let activity = if mode == "-" && map == "-" {
        "-".to_string()
    } else {
        format!("{mode} / {map}")
    };
    let header = if routine {
        "🔔 *FF ONLINE CHECK (RUTIN)*"
    } else {
        "🔍 *FF ONLINE CHECK*"
    };
    let mut out = format!(
        "{header}\n\
         • ID     : `{}`\n\
         • Nick   : {}\n\
         • Status : {badge} *{status}*\n\
         • Region : {} | Level: {}\n\
         • Mode   : {}\n\
         • Cek    : {}\n",
        res.player_id,
        field(&res.profile, "nickname"),
        field(&res.profile, "region"),
        res.profile.get("level").map(|x| x.to_string()).unwrap_or_else(|| "-".to_string()),
        activity,
        field(&res.online, "checkedAt"),
    );
    if let Some(j) = jadwal {
        out.push_str(&format!("• Jadwal : {j}\n"));
    }
    if let Some(summary) = res.online.get("summary").and_then(|x| x.as_str()) {
        out.push_str(&format!("\n_{summary}_"));
    }
    out
}

fn owner_jid(num: &str) -> Option<Jid> {
    let digits: String = num.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    Jid::from_str(&format!("{digits}@s.whatsapp.net")).ok()
}

/// Scheduler rutin: kirim notifikasi ke DM owner setiap selesai pengecekan.
pub async fn run_scheduler(
    client_slot: Arc<Mutex<Option<Arc<whatsapp_rust::Client>>>>,
    schedules: Arc<RwLock<Vec<OnlineSchedule>>>,
    owners: Arc<RwLock<Vec<String>>>,
) {
    loop {
        let now = now_secs();
        let due = {
            let s = schedules.read().unwrap_or_else(|e| e.into_inner());
            s.iter()
                .filter(|x| x.next_run <= now)
                .min_by_key(|x| x.next_run)
                .map(|x| (x.id.clone(), x.interval_secs, x.next_run))
        };

        let Some((id, interval, expected_next)) = due else {
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        };

        // Update dulu supaya tidak dobel eksekusi kalau ada beberapa jadwal.
        {
            let mut s = schedules.write().unwrap_or_else(|e| e.into_inner());
            if let Some(item) = s.iter_mut().find(|x| x.id == id && x.next_run == expected_next) {
                item.next_run = now.saturating_add(interval);
            } else {
                continue;
            }
            save_schedules(&s);
        }

        let jadwal = format!("{} · next +{}", fmt_interval(interval), fmt_interval(interval));
        let text = match freefire_online::check_online(&id).await {
            Ok(res) => format_result(&res, true, Some(&jadwal)),
            Err(e) => format!(
                "🔔 *FF ONLINE CHECK (RUTIN)*\n• ID: `{id}`\n• Status: ⚠️ GAGAL\n• Error: {e}"
            ),
        };

        let mut nums: Vec<String> = owners
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Ok(env) = std::env::var("OWNER_NUMBER") {
            let clean: String = env.chars().filter(|c| c.is_ascii_digit()).collect();
            if !clean.is_empty() {
                nums.push(clean);
            }
        }
        nums.sort();
        nums.dedup();

        let client = client_slot.lock().ok().and_then(|g| g.clone());
        if let Some(client) = client {
            for num in nums {
                if let Some(jid) = owner_jid(&num) {
                    if let Err(e) = client.send_text(jid, text.clone()).await {
                        eprintln!("[ONLINE] gagal kirim notif ke {num}: {e}");
                    }
                }
            }
        } else {
            eprintln!("[ONLINE] scheduler belum punya client, notif dilewati");
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

pub struct Online;

impl Command for Online {
    fn name(&self) -> &'static str {
        "online"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["ffonline", "ff"]
    }

    fn description(&self) -> &'static str {
        "Cek status online Free Fire by ID (owner only)"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            if !is_owner(ctx, bot).await {
                return Ok(());
            }

            let args = args.trim();
            if args.is_empty() {
                ctx.reply_quoting(
                    "Format:\n\
                     • `/online <id>` — cek manual\n\
                     • `/online watch <id> <interval>` — jadwalkan (cth: 15m, 2h)\n\
                     • `/online watch list` — lihat jadwal\n\
                     • `/online watch stop <id|all>` — hapus jadwal\n\
                     Interval minimal 3m, maksimal 1d.",
                )
                .await?;
                return Ok(());
            }

            let mut parts = args.split_whitespace();
            let first = parts.next().unwrap_or("");

            if first.eq_ignore_ascii_case("watch") {
                let rest: Vec<&str> = parts.collect();
                if rest.is_empty() || rest[0].eq_ignore_ascii_case("list") {
                    let s = bot.schedules.read().unwrap_or_else(|e| e.into_inner());
                    if s.is_empty() {
                        ctx.reply_quoting("Belum ada jadwal online check.").await?;
                    } else {
                        let now = now_secs();
                        let mut out = String::from("📋 *Jadwal Online Check*\n");
                        for (i, item) in s.iter().enumerate() {
                            let wait = item.next_run.saturating_sub(now);
                            out.push_str(&format!(
                                "{}) `{}` — {} — next {} detik\n",
                                i + 1,
                                item.id,
                                fmt_interval(item.interval_secs),
                                wait
                            ));
                        }
                        ctx.reply_quoting(out).await?;
                    }
                    return Ok(());
                }

                if rest[0].eq_ignore_ascii_case("stop") {
                    let target = rest.get(1).copied().unwrap_or("");
                    if target.is_empty() {
                        ctx.reply_quoting("Format: `/online watch stop <id|all>`").await?;
                        return Ok(());
                    }
                    let removed = {
                        let mut s = bot.schedules.write().unwrap_or_else(|e| e.into_inner());
                        let before = s.len();
                        if target.eq_ignore_ascii_case("all") {
                            s.clear();
                        } else {
                            s.retain(|x| x.id != target);
                        }
                        save_schedules(&s);
                        before - s.len()
                    };
                    ctx.reply_quoting(format!("{removed} jadwal dihapus.")).await?;
                    return Ok(());
                }

                let id = rest[0];
                if !valid_id(id) {
                    ctx.reply_quoting("ID tidak valid (8-20 digit).").await?;
                    return Ok(());
                }
                let Some(interval) = rest.get(1).and_then(|v| parse_interval(v)) else {
                    ctx.reply_quoting("Interval tidak valid. Contoh: `15m`, `2h`, `30` (=30m).").await?;
                    return Ok(());
                };
                if !(MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&interval) {
                    ctx.reply_quoting(format!(
                        "Interval harus antara {} dan {}.",
                        fmt_interval(MIN_INTERVAL_SECS),
                        fmt_interval(MAX_INTERVAL_SECS)
                    ))
                    .await?;
                    return Ok(());
                }

                let next_run = now_secs().saturating_add(interval);
                {
                    let mut s = bot.schedules.write().unwrap_or_else(|e| e.into_inner());
                    s.retain(|x| x.id != id);
                    s.push(OnlineSchedule {
                        id: id.to_string(),
                        interval_secs: interval,
                        next_run,
                    });
                    save_schedules(&s);
                }
                ctx.reply_quoting(format!(
                    "✅ Jadwal ditambahkan: `{id}` tiap {}. Notifikasi dikirim ke DM owner.",
                    fmt_interval(interval)
                ))
                .await?;
                return Ok(());
            }

            if !valid_id(first) {
                ctx.reply_quoting("ID tidak valid (8-20 digit).").await?;
                return Ok(());
            }

            match freefire_online::check_online(first).await {
                Ok(res) => {
                    ctx.reply_quoting(format_result(&res, false, None)).await?;
                }
                Err(e) => {
                    ctx.reply_quoting(format!("⚠️ Gagal cek `{first}`: {e}")).await?;
                }
            }
            Ok(())
        })
    }
}