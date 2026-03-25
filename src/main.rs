mod browser;
mod config;
mod report;
mod voter;

use clap::Parser;
use config::{Cli, Config};

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
