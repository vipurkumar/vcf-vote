use chromiumoxide::page::Page;
use std::fmt;

use crate::browser::human_pause;
use crate::config::Config;

#[derive(Debug, Clone)]
pub enum VoteStatus {
    Voted,
    AlreadyVoted,
    DryRunOk,
    #[allow(dead_code)]
    ProxyAdded,
    Failed(String),
}

impl fmt::Display for VoteStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoteStatus::Voted => write!(f, "voted"),
            VoteStatus::AlreadyVoted => write!(f, "already_voted"),
            VoteStatus::DryRunOk => write!(f, "dry_run_ok"),
            VoteStatus::ProxyAdded => write!(f, "proxy_added"),
            VoteStatus::Failed(msg) => write!(f, "failed: {}", msg),
        }
    }
}

#[derive(Debug, Clone)]
pub struct VoteResult {
    pub fr_id: String,
    pub url: String,
    pub status: VoteStatus,
    pub message: String,
    pub timestamp: String,
}

/// Attempt to vote on a single feature request.
pub async fn vote_one(
    page: &Page,
    fr_id: &str,
    config: &Config,
) -> VoteResult {
    let url = Config::fr_url(fr_id);
    let timestamp = chrono::Utc::now().to_rfc3339();

    eprintln!("[vote] Navigating to {} ...", url);

    let goto_result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        page.goto(&url),
    ).await;
    if let Err(e) = goto_result.map_err(|_| "Navigation timed out".to_string()).and_then(|r| r.map_err(|e| e.to_string())) {
        return VoteResult {
            fr_id: fr_id.to_string(),
            url,
            status: VoteStatus::Failed(e.to_string()),
            message: "Failed to navigate to FR page".into(),
            timestamp,
        };
    }

    // Wait a moment for dynamic content to load
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Get page content for selector detection
    let html = match page.content().await {
        Ok(h) => h,
        Err(e) => {
            return VoteResult {
                fr_id: fr_id.to_string(),
                url,
                status: VoteStatus::Failed(e.to_string()),
                message: "Failed to read page content".into(),
                timestamp,
            };
        }
    };

    let html_lower = html.to_lowercase();

    // Check if already voted
    let already_voted = html_lower.contains("voted")
        || html_lower.contains("remove vote")
        || html_lower.contains("you voted");

    if already_voted {
        eprintln!("[vote] {} — already voted", fr_id);

        // Attempt proxy vote even if already voted
        if config.company.is_some() {
            let proxy_msg = attempt_proxy_vote(page, config).await;
            return VoteResult {
                fr_id: fr_id.to_string(),
                url,
                status: VoteStatus::AlreadyVoted,
                message: format!("already voted; proxy: {}", proxy_msg),
                timestamp,
            };
        }

        return VoteResult {
            fr_id: fr_id.to_string(),
            url,
            status: VoteStatus::AlreadyVoted,
            message: "Vote control indicates already voted".into(),
            timestamp,
        };
    }

    // Try to find vote button via JavaScript
    let vote_button_found = find_vote_button(page).await;

    if !vote_button_found {
        return VoteResult {
            fr_id: fr_id.to_string(),
            url,
            status: VoteStatus::Failed("vote button not found".into()),
            message: "Could not locate a Vote button on the page. Selectors may need updating.".into(),
            timestamp,
        };
    }

    if config.dry_run {
        eprintln!("[vote] {} — DRY RUN: vote button located, not clicking", fr_id);
        return VoteResult {
            fr_id: fr_id.to_string(),
            url,
            status: VoteStatus::DryRunOk,
            message: "Vote button found; dry run — no click".into(),
            timestamp,
        };
    }

    // Live mode: confirm with human before clicking
    human_pause(&format!(
        "About to click Vote on {}.\nConfirm you want to proceed.",
        fr_id
    ));

    match click_vote_button(page).await {
        Ok(_) => {
            eprintln!("[vote] {} — vote clicked", fr_id);

            let proxy_msg = if config.company.is_some() {
                attempt_proxy_vote(page, config).await
            } else {
                "proxy vote not configured".into()
            };

            VoteResult {
                fr_id: fr_id.to_string(),
                url,
                status: VoteStatus::Voted,
                message: format!("vote clicked; proxy: {}", proxy_msg),
                timestamp,
            }
        }
        Err(e) => VoteResult {
            fr_id: fr_id.to_string(),
            url,
            status: VoteStatus::Failed(e.to_string()),
            message: "Failed to click vote button".into(),
            timestamp,
        },
    }
}

/// Use JavaScript to find a vote button on the page.
async fn find_vote_button(page: &Page) -> bool {
    let js = r#"
        (function() {
            const buttons = document.querySelectorAll('button, a, [role="button"]');
            for (const btn of buttons) {
                const text = (btn.textContent || '').trim().toLowerCase();
                if (text === 'vote' || text === '+1' || text.includes('vote for this')) {
                    return true;
                }
            }
            return false;
        })()
    "#;

    match page.evaluate(js).await {
        Ok(val) => val.into_value::<bool>().unwrap_or(false),
        Err(_) => false,
    }
}

/// Use JavaScript to click the vote button.
async fn click_vote_button(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let js = r#"
        (function() {
            const buttons = document.querySelectorAll('button, a, [role="button"]');
            for (const btn of buttons) {
                const text = (btn.textContent || '').trim().toLowerCase();
                if (text === 'vote' || text === '+1' || text.includes('vote for this')) {
                    btn.click();
                    return true;
                }
            }
            return false;
        })()
    "#;

    let result = page.evaluate(js).await?;
    let clicked = result.into_value::<bool>().unwrap_or(false);

    if clicked {
        // Wait for the vote action to register
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        Ok(())
    } else {
        Err("Vote button not found during click attempt".into())
    }
}

/// Attempt to add a proxy vote (company + use case).
async fn attempt_proxy_vote(page: &Page, config: &Config) -> String {
    let company = match &config.company {
        Some(c) => c.clone(),
        None => return "proxy vote not configured".into(),
    };
    let use_case = config.use_case.clone().unwrap_or_default();

    // Try to find and open a proxy vote / "add details" dialog
    let js_open = r#"
        (function() {
            const buttons = document.querySelectorAll('button, a, [role="button"]');
            for (const btn of buttons) {
                const text = (btn.textContent || '').trim().toLowerCase();
                if (text.includes('proxy') || text.includes('add details') || text.includes('on behalf')) {
                    btn.click();
                    return true;
                }
            }
            return false;
        })()
    "#;

    match page.evaluate(js_open).await {
        Ok(val) => {
            let opened = val.into_value::<bool>().unwrap_or(false);
            if !opened {
                return "proxy vote dialog not found — selectors may need updating".into();
            }
        }
        Err(e) => return format!("error opening proxy dialog: {}", e),
    }

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Try to fill company and use case fields
    let js_fill = format!(
        r#"
        (function() {{
            const inputs = document.querySelectorAll('input, textarea');
            let filled = 0;
            for (const input of inputs) {{
                const label = (input.getAttribute('aria-label') || input.getAttribute('placeholder') || '').toLowerCase();
                if (label.includes('company') || label.includes('organization')) {{
                    input.value = {company};
                    input.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    filled++;
                }}
                if (label.includes('use case') || label.includes('reason') || label.includes('details')) {{
                    input.value = {use_case};
                    input.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    filled++;
                }}
            }}
            return filled;
        }})()
        "#,
        company = serde_json::to_string(&company).unwrap_or_default(),
        use_case = serde_json::to_string(&use_case).unwrap_or_default(),
    );

    match page.evaluate(js_fill.as_str()).await {
        Ok(val) => {
            let filled = val.into_value::<i64>().unwrap_or(0);
            if filled == 0 {
                return "proxy fields not found — selectors may need updating".into();
            }
        }
        Err(e) => return format!("error filling proxy fields: {}", e),
    }

    // Always pause for human review before submitting proxy vote
    human_pause(
        "Review the proxy vote text in the browser dialog.\n\
         Make any edits needed, then press ENTER to submit.",
    );

    // Click submit
    let js_submit = r#"
        (function() {
            const buttons = document.querySelectorAll('button, [role="button"]');
            for (const btn of buttons) {
                const text = (btn.textContent || '').trim().toLowerCase();
                if (text.includes('submit') || text.includes('save') || text.includes('add vote')) {
                    btn.click();
                    return true;
                }
            }
            return false;
        })()
    "#;

    match page.evaluate(js_submit).await {
        Ok(val) => {
            let submitted = val.into_value::<bool>().unwrap_or(false);
            if submitted {
                "proxy vote submitted".into()
            } else {
                "proxy submit button not found — may need manual submission".into()
            }
        }
        Err(e) => format!("error submitting proxy vote: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vote_status_display() {
        assert_eq!(VoteStatus::Voted.to_string(), "voted");
        assert_eq!(VoteStatus::AlreadyVoted.to_string(), "already_voted");
        assert_eq!(VoteStatus::DryRunOk.to_string(), "dry_run_ok");
        assert_eq!(VoteStatus::ProxyAdded.to_string(), "proxy_added");
        assert_eq!(
            VoteStatus::Failed("timeout".into()).to_string(),
            "failed: timeout"
        );
    }

    #[test]
    fn test_vote_result_construction() {
        let result = VoteResult {
            fr_id: "FR-1234".to_string(),
            url: "https://example.com/fr/1234".to_string(),
            status: VoteStatus::Voted,
            message: "vote clicked successfully".to_string(),
            timestamp: "2026-03-25T00:00:00Z".to_string(),
        };

        assert_eq!(result.fr_id, "FR-1234");
        assert_eq!(result.url, "https://example.com/fr/1234");
        assert_eq!(result.status.to_string(), "voted");
        assert_eq!(result.message, "vote clicked successfully");
        assert_eq!(result.timestamp, "2026-03-25T00:00:00Z");
    }
}
