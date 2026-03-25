use clap::Parser;

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    RegisterAndVote,
    VoteOnly,
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "register_and_vote" | "register-and-vote" => Ok(Mode::RegisterAndVote),
            "vote_only" | "vote-only" => Ok(Mode::VoteOnly),
            _ => Err(format!("Invalid mode: '{}'. Use 'vote_only' or 'register_and_vote'", s)),
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::RegisterAndVote => write!(f, "register_and_vote"),
            Mode::VoteOnly => write!(f, "vote_only"),
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "vcf-vote", about = "VCF Voting Assistant — human-in-the-loop browser automation for Aha! Ideas portal")]
pub struct Cli {
    /// Comma-separated Feature Request IDs (e.g. VCF-I-979,VCF-I-3704)
    #[arg(long = "ids", value_delimiter = ',')]
    pub ids: Vec<String>,

    /// Mode: "vote_only" or "register_and_vote"
    #[arg(long, default_value = "vote_only")]
    pub mode: Mode,

    /// Dry run: validate URLs and selectors without clicking Vote
    #[arg(long, default_value_t = true)]
    pub dry_run: bool,

    /// Actually vote (disables dry-run)
    #[arg(long, default_value_t = false)]
    pub live: bool,

    /// Company name for proxy vote prefill
    #[arg(long)]
    pub company: Option<String>,

    /// Use case summary for proxy vote prefill
    #[arg(long)]
    pub use_case: Option<String>,

    /// CSV report output path
    #[arg(long, default_value = "vcf_votes_report.csv")]
    pub output: String,

}

#[derive(Debug)]
pub struct Config {
    pub fr_ids: Vec<String>,
    pub mode: Mode,
    pub dry_run: bool,
    pub company: Option<String>,
    pub use_case: Option<String>,
    pub output_path: String,
}

pub const PORTAL_URL: &str = "https://vcf.ideas.aha.io";
pub const REGISTRATION_URL: &str = "https://profile.broadcom.com/web/registration";

impl Config {
    pub fn from_cli(cli: Cli) -> Result<Self, String> {
        if cli.ids.is_empty() {
            return Err("No feature request IDs provided. Use --ids VCF-I-979,VCF-I-3704".into());
        }

        for id in &cli.ids {
            if !is_valid_fr_id(id) {
                return Err(format!(
                    "Invalid FR ID: '{}'. Expected format: VCF-I-<number> (e.g. VCF-I-979)",
                    id
                ));
            }
        }

        let dry_run = if cli.live { false } else { cli.dry_run };

        Ok(Config {
            fr_ids: cli.ids,
            mode: cli.mode,
            dry_run,
            company: cli.company,
            use_case: cli.use_case,
            output_path: cli.output,
        })
    }

    pub fn fr_url(fr_id: &str) -> String {
        format!("{}/ideas/{}", PORTAL_URL, fr_id)
    }
}

fn is_valid_fr_id(id: &str) -> bool {
    if let Some(suffix) = id.strip_prefix("VCF-I-") {
        !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_fr_ids() {
        assert!(is_valid_fr_id("VCF-I-979"));
        assert!(is_valid_fr_id("VCF-I-3704"));
        assert!(!is_valid_fr_id("VCF-979"));
        assert!(!is_valid_fr_id("VCF-I-"));
        assert!(!is_valid_fr_id("VCF-I-abc"));
        assert!(!is_valid_fr_id(""));
    }

    #[test]
    fn test_fr_url() {
        assert_eq!(
            Config::fr_url("VCF-I-979"),
            "https://vcf.ideas.aha.io/ideas/VCF-I-979"
        );
    }

    #[test]
    fn test_mode_parsing() {
        assert_eq!("vote_only".parse::<Mode>().unwrap(), Mode::VoteOnly);
        assert_eq!("vote-only".parse::<Mode>().unwrap(), Mode::VoteOnly);
        assert_eq!("register_and_vote".parse::<Mode>().unwrap(), Mode::RegisterAndVote);
        assert!("invalid".parse::<Mode>().is_err());
    }

    #[test]
    fn test_mode_display() {
        assert_eq!(Mode::VoteOnly.to_string(), "vote_only");
        assert_eq!(Mode::RegisterAndVote.to_string(), "register_and_vote");
    }

    #[test]
    fn test_config_from_cli_valid() {
        let cli = Cli {
            ids: vec!["VCF-I-979".into()],
            mode: Mode::VoteOnly,
            dry_run: true,
            live: false,
            company: None,
            use_case: None,
            output: "test.csv".into(),
        };
        let config = Config::from_cli(cli).unwrap();
        assert_eq!(config.fr_ids, vec!["VCF-I-979".to_string()]);
        assert_eq!(config.mode, Mode::VoteOnly);
        assert!(config.dry_run);
        assert_eq!(config.output_path, "test.csv");
    }

    #[test]
    fn test_config_from_cli_empty_ids() {
        let cli = Cli {
            ids: vec![],
            mode: Mode::VoteOnly,
            dry_run: true,
            live: false,
            company: None,
            use_case: None,
            output: "test.csv".into(),
        };
        let err = Config::from_cli(cli).unwrap_err();
        assert!(err.contains("No feature request IDs"), "Expected error about empty IDs, got: {}", err);
    }

    #[test]
    fn test_config_from_cli_invalid_id() {
        let cli = Cli {
            ids: vec!["INVALID".into()],
            mode: Mode::VoteOnly,
            dry_run: true,
            live: false,
            company: None,
            use_case: None,
            output: "test.csv".into(),
        };
        let err = Config::from_cli(cli).unwrap_err();
        assert!(err.contains("Invalid FR ID"), "Expected error about invalid ID, got: {}", err);
    }

    #[test]
    fn test_config_from_cli_live_overrides_dry_run() {
        let cli = Cli {
            ids: vec!["VCF-I-979".into()],
            mode: Mode::VoteOnly,
            dry_run: true,
            live: true,
            company: None,
            use_case: None,
            output: "test.csv".into(),
        };
        let config = Config::from_cli(cli).unwrap();
        assert!(!config.dry_run, "live=true should override dry_run to false");
    }

    #[test]
    fn test_config_from_cli_company_passthrough() {
        let cli = Cli {
            ids: vec!["VCF-I-979".into()],
            mode: Mode::VoteOnly,
            dry_run: true,
            live: false,
            company: Some("ING".into()),
            use_case: Some("test".into()),
            output: "test.csv".into(),
        };
        let config = Config::from_cli(cli).unwrap();
        assert_eq!(config.company, Some("ING".to_string()));
        assert_eq!(config.use_case, Some("test".to_string()));
    }
}
