mod browser;
mod config;
mod report;
mod tui;
mod voter;

use std::sync::{Arc, Mutex};

use clap::Parser;
use config::{Cli, Config};
use tui::{AppPhase, AppState, SharedState};

const DISCLAIMER: &str = "\
╔══════════════════════════════════════════════════════════════════╗
║                  VCF VOTING ASSISTANT                           ║
║                                                                 ║
║  This tool assists with navigating and voting on VCF feature    ║
║  requests on Broadcom's Aha! Ideas portal.                      ║
║                                                                 ║
║  IMPORTANT:                                                     ║
║  • You must have permission to automate navigation/clicks       ║
║  • You must have read the portal Terms of Use                   ║
║  • You will perform all login/2FA/CAPTCHA steps manually        ║
║  • This tool does NOT store or handle credentials               ║
║  • All vote clicks require your explicit confirmation            ║
╚══════════════════════════════════════════════════════════════════╝";

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let config = match Config::from_cli(cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    if config.no_tui {
        run_plain(config).await;
    } else {
        run_with_tui(config).await;
    }
}

// ── TUI mode ────────────────────────────────────────────────────────────────

async fn run_with_tui(config: Config) {
    let state: SharedState = Arc::new(Mutex::new(AppState::new(
        &config.fr_ids,
        config.dry_run,
        &config.mode.to_string(),
        config.company.clone(),
    )));

    let worker_state = state.clone();
    let tui_state = state.clone();

    let result = tokio::select! {
        tui_result = tui::run_tui_loop(tui_state) => {
            tui_result.map_err(|e| format!("TUI error: {}", e))
        }
        worker_result = run_worker(worker_state, config) => {
            worker_result
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

async fn run_worker(state: SharedState, config: Config) -> Result<(), String> {
    // Wait for disclaimer acceptance
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if state.lock().unwrap().phase != AppPhase::Disclaimer {
            break;
        }
        if state.lock().unwrap().should_quit {
            return Ok(());
        }
    }

    // Launch browser
    state.lock().unwrap().log("Launching browser...");
    let (_browser, page) = match browser::launch_browser().await {
        Ok(bp) => bp,
        Err(e) => {
            let msg = format!("Error launching browser: {}. Make sure Chromium/Chrome is installed.", e);
            state.lock().unwrap().log(format!("ERROR: {}", msg));
            state.lock().unwrap().phase = AppPhase::Done;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            return Err(msg);
        }
    };

    // Auth gate
    {
        state.lock().unwrap().phase = AppPhase::Auth;
        state.lock().unwrap().log("Starting authentication...");
    }
    if let Err(e) = browser::auth_gate_tui(&page, &config, &state).await {
        let msg = format!("Error during authentication: {}", e);
        state.lock().unwrap().log(format!("ERROR: {}", msg));
        state.lock().unwrap().phase = AppPhase::Done;
        return Err(msg);
    }

    // Voting
    {
        state.lock().unwrap().phase = AppPhase::Voting;
        state.lock().unwrap().log("Authentication complete. Starting vote process...");
    }

    let mut results = Vec::new();
    for (index, fr_id) in config.fr_ids.iter().enumerate() {
        if state.lock().unwrap().should_quit {
            state.lock().unwrap().log("User requested quit — stopping.");
            break;
        }
        let result = voter::vote_one_tui(&page, fr_id, index, &config, &state).await;
        results.push(result);
    }

    // Write report
    if let Err(e) = report::write_csv(&results, &config.output_path) {
        state.lock().unwrap().log(format!("ERROR writing CSV report: {}", e));
    } else {
        state.lock().unwrap().log(format!("CSV report written to {}", config.output_path));
    }

    // Summary
    {
        let mut s = state.lock().unwrap();
        let total = results.len();
        let voted = results.iter().filter(|r| matches!(r.status, voter::VoteStatus::Voted)).count();
        let already = results.iter().filter(|r| matches!(r.status, voter::VoteStatus::AlreadyVoted)).count();
        let dry_ok = results.iter().filter(|r| matches!(r.status, voter::VoteStatus::DryRunOk)).count();
        let failed = results.iter().filter(|r| matches!(r.status, voter::VoteStatus::Failed(_))).count();
        s.log(format!("Done! Total={} Voted={} Already={} DryOK={} Failed={}", total, voted, already, dry_ok, failed));
        s.phase = AppPhase::Done;
    }

    // Wait for user to quit via TUI
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if state.lock().unwrap().should_quit {
            break;
        }
    }

    Ok(())
}

// ── Plain (no-TUI) mode ─────────────────────────────────────────────────────

async fn run_plain(config: Config) {
    eprintln!("{}", DISCLAIMER);
    eprintln!();
    eprintln!("Mode:    {}", config.mode);
    eprintln!("Dry run: {}", config.dry_run);
    eprintln!("FRs:     {:?}", config.fr_ids);
    if let Some(ref company) = config.company {
        eprintln!("Company: {}", company);
    }
    eprintln!();

    browser::human_pause(
        "By pressing ENTER you confirm:\n\
         1. You have permission to automate navigation on this portal\n\
         2. You have read the relevant Terms of Use and policies\n\
         3. You consent to the actions this tool will take on your behalf",
    );

    eprintln!("[main] Launching browser...");
    let (_browser, page) = match browser::launch_browser().await {
        Ok(bp) => bp,
        Err(e) => {
            eprintln!("Error launching browser: {}", e);
            eprintln!("Make sure Chromium/Chrome is installed.");
            std::process::exit(1);
        }
    };

    if let Err(e) = browser::auth_gate(&page, &config).await {
        eprintln!("Error during authentication: {}", e);
        std::process::exit(1);
    }

    let mut results = Vec::new();
    for fr_id in &config.fr_ids {
        let result = voter::vote_one(&page, fr_id, &config).await;
        eprintln!("[main] {} → {} ({})", result.fr_id, result.status, result.message);
        results.push(result);
    }

    if let Err(e) = report::write_csv(&results, &config.output_path) {
        eprintln!("Error writing CSV report: {}", e);
    }

    report::print_summary(&results);
}
