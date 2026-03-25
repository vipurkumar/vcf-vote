use std::path::Path;

use crate::voter::{VoteResult, VoteStatus};

/// Write vote results to a CSV file.
pub fn write_csv(results: &[VoteResult], path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = csv::Writer::from_path(Path::new(path))?;
    writer.write_record(["fr_id", "url", "status", "message", "timestamp"])?;

    for r in results {
        writer.write_record([
            &r.fr_id,
            &r.url,
            &r.status.to_string(),
            &r.message,
            &r.timestamp,
        ])?;
    }

    writer.flush()?;
    eprintln!("[report] CSV written to {}", path);
    Ok(())
}

/// Print a markdown summary of results to stdout.
#[allow(dead_code)]
pub fn print_summary(results: &[VoteResult]) {
    let total = results.len();
    let voted = results.iter().filter(|r| matches!(r.status, VoteStatus::Voted)).count();
    let already = results.iter().filter(|r| matches!(r.status, VoteStatus::AlreadyVoted)).count();
    let dry_run = results.iter().filter(|r| matches!(r.status, VoteStatus::DryRunOk)).count();
    let failed = results.iter().filter(|r| matches!(r.status, VoteStatus::Failed(_))).count();

    println!();
    println!("## VCF Vote Summary");
    println!();
    println!("| Metric       | Count |");
    println!("|--------------|-------|");
    println!("| Total FRs    | {:<5} |", total);
    println!("| Voted        | {:<5} |", voted);
    println!("| Already voted| {:<5} |", already);
    println!("| Dry run OK   | {:<5} |", dry_run);
    println!("| Failed       | {:<5} |", failed);
    println!();

    if failed > 0 {
        println!("### Failures");
        for r in results.iter().filter(|r| matches!(r.status, VoteStatus::Failed(_))) {
            println!("- **{}**: {}", r.fr_id, r.message);
        }
        println!();
    }

    for r in results {
        println!("- {} → {} ({})", r.fr_id, r.status, r.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voter::{VoteResult, VoteStatus};

    #[test]
    fn test_write_csv_creates_file() {
        let results = vec![
            VoteResult {
                fr_id: "FR-1001".to_string(),
                url: "https://example.com/fr/1001".to_string(),
                status: VoteStatus::Voted,
                message: "vote clicked".to_string(),
                timestamp: "2026-03-25T00:00:00Z".to_string(),
            },
            VoteResult {
                fr_id: "FR-2002".to_string(),
                url: "https://example.com/fr/2002".to_string(),
                status: VoteStatus::AlreadyVoted,
                message: "already voted".to_string(),
                timestamp: "2026-03-25T01:00:00Z".to_string(),
            },
        ];

        let path = "/tmp/vcf_test_report.csv";
        write_csv(&results, path).expect("write_csv should succeed");

        assert!(Path::new(path).exists(), "CSV file should exist");

        let contents = std::fs::read_to_string(path).expect("should read CSV file");
        assert!(
            contents.contains("fr_id,url,status,message,timestamp"),
            "CSV should contain header"
        );
        assert!(contents.contains("FR-1001"), "CSV should contain FR-1001");
        assert!(contents.contains("FR-2002"), "CSV should contain FR-2002");

        // Clean up
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_write_csv_empty_results() {
        let results: Vec<VoteResult> = vec![];

        let path = "/tmp/vcf_test_report_empty.csv";
        write_csv(&results, path).expect("write_csv should succeed for empty results");

        assert!(Path::new(path).exists(), "CSV file should exist");

        let contents = std::fs::read_to_string(path).expect("should read CSV file");
        let lines: Vec<&str> = contents.trim().lines().collect();
        assert_eq!(lines.len(), 1, "CSV should contain only the header line");
        assert!(
            lines[0].contains("fr_id,url,status,message,timestamp"),
            "CSV should contain header"
        );

        // Clean up
        let _ = std::fs::remove_file(path);
    }
}
