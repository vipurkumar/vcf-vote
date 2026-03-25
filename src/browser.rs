use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::Page;
use futures::StreamExt;
use std::io::{self, Write};
use std::time::Duration;

use crate::config::{Config, Mode, PORTAL_URL, REGISTRATION_URL};
use crate::tui::SharedState;

/// Print a message and wait for the human to press ENTER.
pub fn human_pause(message: &str) {
    eprintln!();
    eprintln!("=== HUMAN ACTION REQUIRED ===");
    eprintln!("{}", message);
    eprint!("Press ENTER to continue once completed...");
    io::stderr().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).ok();
}

/// TUI-aware version of `human_pause`: sets a modal on the shared TUI state
/// and waits for the user to press ENTER in the TUI.
pub async fn human_pause_tui(state: &SharedState, message: &str) {
    {
        let mut s = state.lock().unwrap();
        s.modal = Some(message.to_string());
        s.modal_confirmed = false;
    }

    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let confirmed = state.lock().unwrap().modal_confirmed;
        if confirmed {
            break;
        }
    }

    {
        let mut s = state.lock().unwrap();
        s.modal = None;
    }
}

/// Find Chrome/Chromium executable, checking CHROME_PATH env var then platform defaults.
fn find_chrome() -> Option<String> {
    if let Ok(p) = std::env::var("CHROME_PATH") {
        return Some(p);
    }

    let candidates = platform_chrome_candidates();
    for c in candidates {
        if std::path::Path::new(&c).exists() {
            return Some(c);
        }
    }

    None // fall through to chromiumoxide's built-in search
}

fn platform_chrome_candidates() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        vec![
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into(),
            "/Applications/Chromium.app/Contents/MacOS/Chromium".into(),
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser".into(),
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into(),
        ]
    }

    #[cfg(target_os = "windows")]
    {
        let program_files = std::env::var("PROGRAMFILES").unwrap_or_else(|_| r"C:\Program Files".into());
        let program_files_x86 = std::env::var("PROGRAMFILES(X86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
        let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\Users\Default".into());
            format!(r"{}\AppData\Local", home)
        });

        vec![
            format!(r"{}\Google\Chrome\Application\chrome.exe", program_files),
            format!(r"{}\Google\Chrome\Application\chrome.exe", program_files_x86),
            format!(r"{}\Google\Chrome\Application\chrome.exe", local_app_data),
            format!(r"{}\Chromium\Application\chrome.exe", program_files),
            format!(r"{}\BraveSoftware\Brave-Browser\Application\brave.exe", program_files),
            format!(r"{}\Microsoft\Edge\Application\msedge.exe", program_files),
            format!(r"{}\Microsoft\Edge\Application\msedge.exe", program_files_x86),
        ]
    }

    #[cfg(target_os = "linux")]
    {
        vec![
            "/usr/bin/google-chrome".into(),
            "/usr/bin/google-chrome-stable".into(),
            "/usr/bin/chromium-browser".into(),
            "/usr/bin/chromium".into(),
            "/snap/bin/chromium".into(),
            "/usr/bin/brave-browser".into(),
            "/usr/bin/microsoft-edge".into(),
        ]
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        vec![]
    }
}

/// Launch a headed (visible) Chromium browser and return the Browser handle + first page.
pub async fn launch_browser() -> Result<(Browser, Page), Box<dyn std::error::Error>> {
    let mut builder = BrowserConfig::builder();
    builder = builder.with_head().window_size(1280, 900);

    // Platform-specific Chrome flags
    #[cfg(target_os = "linux")]
    {
        builder = builder
            .no_sandbox()
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-setuid-sandbox");
    }

    #[cfg(target_os = "macos")]
    {
        builder = builder.arg("--disable-gpu");
    }

    #[cfg(target_os = "windows")]
    {
        builder = builder
            .no_sandbox()
            .arg("--disable-gpu");
    }

    if let Some(chrome_path) = find_chrome() {
        builder = builder.chrome_executable(chrome_path);
    }

    let (browser, mut handler) = Browser::launch(
        builder
            .build()
            .map_err(|e| format!("Failed to build browser config: {}", e))?,
    )
    .await?;

    // Spawn the handler loop so CDP messages are processed
    tokio::spawn(async move {
        while let Some(_event) = handler.next().await {}
    });

    let page = browser.new_page("about:blank").await?;
    Ok((browser, page))
}

/// Handle the authentication gate: open portal (and registration page if needed),
/// then pause for the human to sign in.
#[allow(dead_code)]
pub async fn auth_gate(page: &Page, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if config.mode == Mode::RegisterAndVote {
        eprintln!("[auth] Opening registration page: {}", REGISTRATION_URL);
        page.goto(REGISTRATION_URL).await?;
        human_pause(
            "Please complete your Broadcom account registration.\n\
             Fill in all required fields and verify your email.\n\
             Once registration is complete, press ENTER.",
        );
    }

    eprintln!("[auth] Opening VCF Ideas portal: {}", PORTAL_URL);
    page.goto(PORTAL_URL).await?;
    human_pause(
        "Please sign in to the Aha! Ideas portal.\n\
         Complete any 2FA or CAPTCHA challenges.\n\
         Once you see your user menu/avatar, press ENTER.",
    );

    // Basic auth verification: check for absence of common sign-in indicators
    let html = page.content().await.unwrap_or_default();
    if html.to_lowercase().contains("sign in") || html.to_lowercase().contains("log in") {
        eprintln!("[auth] WARNING: Page still appears to show a sign-in prompt.");
        eprintln!("[auth] If you are actually logged in, this may be a false positive.");
        human_pause("Verify you are logged in, then press ENTER to continue anyway.");
    } else {
        eprintln!("[auth] Auth check passed — no sign-in prompt detected.");
    }

    Ok(())
}

/// TUI-aware version of `auth_gate`: uses the TUI modal for human interaction
/// and logs messages to the shared TUI state instead of stderr.
pub async fn auth_gate_tui(
    page: &Page,
    config: &Config,
    state: &SharedState,
) -> Result<(), Box<dyn std::error::Error>> {
    if config.mode == Mode::RegisterAndVote {
        state.lock().unwrap().log(format!(
            "[auth] Opening registration page: {}",
            REGISTRATION_URL
        ));
        page.goto(REGISTRATION_URL).await?;
        human_pause_tui(
            state,
            "Please complete your Broadcom account registration.\n\
             Fill in all required fields and verify your email.\n\
             Once registration is complete, press ENTER.",
        )
        .await;
    }

    state.lock().unwrap().log(format!(
        "[auth] Opening VCF Ideas portal: {}",
        PORTAL_URL
    ));
    page.goto(PORTAL_URL).await?;
    human_pause_tui(
        state,
        "Please sign in to the Aha! Ideas portal.\n\
         Complete any 2FA or CAPTCHA challenges.\n\
         Once you see your user menu/avatar, press ENTER.",
    )
    .await;

    // Basic auth verification: check for absence of common sign-in indicators
    let html = page.content().await.unwrap_or_default();
    if html.to_lowercase().contains("sign in") || html.to_lowercase().contains("log in") {
        state.lock().unwrap().log(
            "[auth] WARNING: Page still appears to show a sign-in prompt.".to_string(),
        );
        state.lock().unwrap().log(
            "[auth] If you are actually logged in, this may be a false positive.".to_string(),
        );
        human_pause_tui(
            state,
            "Verify you are logged in, then press ENTER to continue anyway.",
        )
        .await;
    } else {
        state
            .lock()
            .unwrap()
            .log("[auth] Auth check passed — no sign-in prompt detected.".to_string());
    }

    Ok(())
}
