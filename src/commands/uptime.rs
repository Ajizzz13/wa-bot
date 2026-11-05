use std::fs;
use std::time::Duration;

use whatsapp_rust::bot::MessageContext;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Uptime;

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let rem_secs = secs % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}h"));
    }
    if hours > 0 {
        parts.push(format!("{hours}j"));
    }
    if mins > 0 {
        parts.push(format!("{mins}m"));
    }
    parts.push(format!("{rem_secs}d"));
    parts.join(" ")
}

fn get_system_uptime() -> Option<String> {
    let content = fs::read_to_string("/proc/uptime").ok()?;
    let first = content.split_whitespace().next()?;
    let secs: f64 = first.parse().ok()?;
    Some(format_duration(Duration::from_secs(secs as u64)))
}

fn get_memory_info() -> Option<String> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb: Option<u64> = None;
    let mut avail_kb: Option<u64> = None;

    for line in content.lines() {
        if line.starts_with("MemTotal:") {
            total_kb = line.split_whitespace().nth(1).and_then(|v| v.parse().ok());
        } else if line.starts_with("MemAvailable:") {
            avail_kb = line.split_whitespace().nth(1).and_then(|v| v.parse().ok());
        }
    }

    match (total_kb, avail_kb) {
        (Some(total), Some(avail)) => {
            let used = total.saturating_sub(avail);
            let used_mb = used / 1024;
            let total_mb = total / 1024;
            let pct = (used as f64 / total as f64) * 100.0;
            Some(format!("{used_mb} MB / {total_mb} MB ({pct:.1}%)"))
        }
        _ => None,
    }
}

fn get_bot_memory() -> String {
    crate::bot_memory_stat()
}
impl Command for Uptime {
    fn name(&self) -> &'static str {
        "uptime"
    }

    fn description(&self) -> &'static str {
        "Status durasi aktif bot dan resource server Colab"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        _args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            let bot_up = format_duration(bot.start_time.elapsed());
            let bot_mem = get_bot_memory();
            let sys_up = get_system_uptime().unwrap_or_else(|| "N/A".to_string());
            let mem = get_memory_info().unwrap_or_else(|| "N/A".to_string());

            let text = format!(
                "⏱️ *Status Uptime & Sistem*\n\n\
                 • *Bot Uptime:* {bot_up}\n\
                 • *Bot RAM:* {bot_mem}\n\
                 • *Server Uptime:* {sys_up}\n\
                 • *Server RAM:* {mem}\n\
                 • *Platform:* Google Colab (Linux x86_64)"
            );

            ctx.reply_quoting(text).await?;
            Ok(())
        })
    }
}
