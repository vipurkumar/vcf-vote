# VCF Vote

### The Feature Request Upvote Machine That Nobody Asked For (But Everyone Needed)

---

> **Origin Story:** Someone on the team forwarded an email: *"Hey, can everyone register and upvote these 5 VCF feature requests?"* A normal person would have spent 3 minutes clicking buttons. Instead, we spent a weekend building a CLI tool to automate it. In Rust. Because of course we did.
>
> What started as *"I'll just automate this real quick"* turned into a full-blown CLI application with cross-platform Chrome detection, proxy vote support, and more error handling than the actual voting portal probably has.
>
> Was it worth it? Absolutely not. Would we do it again? Already planning v2.

---

## What Is This?

A **human-in-the-loop** CLI tool that helps you navigate and upvote VMware Cloud Foundation (VCF) feature requests on [Broadcom's Aha! Ideas portal](https://vcf.ideas.aha.io). It opens a real browser, you log in like a normal human being, and then it clicks the vote buttons for you.

**It does NOT:**
- Store your credentials (we're not animals)
- Bypass CAPTCHAs (we're not *those* kind of engineers)
- Do anything without your explicit consent (every click requires you to press ENTER)
- Replace the 3 minutes it would have taken to just do it manually

**It DOES:**
- Look really cool in your terminal
- Generate a CSV report so you can prove to management that you voted
- Make you mass-upvote VMware feature requests like a civic duty

---

## Installation

### Prerequisites

- **Rust** (1.70+)
- **Chrome** or **Chromium** (the tool finds it automatically)
- **A Broadcom/Aha! account** (the one thing we can't automate)

### Install Rust

<details>
<summary><b>macOS</b></summary>

```bash
# Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Restart your terminal, or:
source "$HOME/.cargo/env"

# Verify
rustc --version
```

Chrome is usually already at `/Applications/Google Chrome.app` — the tool finds it automatically.

</details>

<details>
<summary><b>Linux</b></summary>

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Make sure Chrome/Chromium is installed
# Ubuntu/Debian:
sudo apt install chromium-browser
# Fedora:
sudo dnf install chromium
# Arch:
sudo pacman -S chromium
```

</details>

<details>
<summary><b>Windows</b></summary>

1. Download and run [rustup-init.exe](https://rustup.rs)
2. Follow the prompts (default options are fine)
3. Restart your terminal
4. Chrome is usually already installed at `C:\Program Files\Google\Chrome\Application\chrome.exe`

If Chrome is in a non-standard location:
```powershell
$env:CHROME_PATH = "C:\path\to\chrome.exe"
```

</details>

---

## Build

```bash
git clone https://github.com/vipurkumar/vcf-vote.git
cd vcf-vote
cargo build --release
```

The binary will be at `target/release/vcf-vote`.

---

## Usage

### Quick Start (macOS)

```bash
# Dry run — opens browser, navigates to FRs, checks selectors, doesn't click anything
cargo run --release -- --ids VCF-I-979,VCF-I-3704,VCF-I-3707,VCF-I-2188,VCF-I-3659
```

Here's what happens:
1. A disclaimer prompt appears. Press **ENTER** to accept.
2. Chrome opens to the Aha! portal.
3. A prompt says **"Please sign in"** — go to the browser, log in, handle any 2FA/CAPTCHA.
4. Press **ENTER** in the terminal when you're logged in.
5. The tool navigates to each feature request page.
6. In dry-run mode, it checks if the Vote button exists and reports back.
7. A CSV report is saved and a summary table is shown.

### Actually Vote (The Whole Point)

```bash
# The --live flag enables actual clicking
cargo run --release -- --ids VCF-I-979,VCF-I-3704,VCF-I-3707,VCF-I-2188,VCF-I-3659 --live
```

In live mode, the tool pauses before **every single vote click** and asks you to confirm. You're always in control.

### Vote on Behalf of Your Company

```bash
cargo run --release -- \
  --ids VCF-I-979,VCF-I-3704,VCF-I-3707,VCF-I-2188,VCF-I-3659 \
  --live \
  --company "Acme Corp" \
  --use-case "Critical for our compliance and operations posture"
```

This attempts to open the proxy vote dialog and prefill your company name and use case. You review the text before it submits.

### Register + Vote (New Account)

```bash
cargo run --release -- \
  --ids VCF-I-979,VCF-I-3704 \
  --mode register_and_vote \
  --live
```

This opens the Broadcom registration page first, waits for you to create an account, then proceeds to the portal.

### Custom Chrome Path

If Chrome isn't auto-detected:

```bash
# macOS
export CHROME_PATH="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

# Linux
export CHROME_PATH="/usr/bin/google-chrome-stable"

# Windows (PowerShell)
$env:CHROME_PATH = "C:\Program Files\Google\Chrome\Application\chrome.exe"

cargo run --release -- --ids VCF-I-979
```

---

## All Options

```
Usage: vcf-vote [OPTIONS]

Options:
      --ids <IDS>            Comma-separated FR IDs (e.g. VCF-I-979,VCF-I-3704)
      --mode <MODE>          "vote_only" or "register_and_vote" [default: vote_only]
      --dry-run              Validate selectors without clicking [default: true]
      --live                 Actually vote (disables dry-run)
      --company <COMPANY>    Company name for proxy vote
      --use-case <USE_CASE>  Use case summary for proxy vote
      --output <OUTPUT>      CSV report path [default: vcf_votes_report.csv]
  -h, --help                 Print help
```

---

## The Feature Requests

These are the VCF feature requests that started this whole adventure:

| ID | Link |
|----|------|
| VCF-I-979 | https://vcf.ideas.aha.io/ideas/VCF-I-979 |
| VCF-I-3704 | https://vcf.ideas.aha.io/ideas/VCF-I-3704 |
| VCF-I-3707 | https://vcf.ideas.aha.io/ideas/VCF-I-3707 |
| VCF-I-2188 | https://vcf.ideas.aha.io/ideas/VCF-I-2188 |
| VCF-I-3659 | https://vcf.ideas.aha.io/ideas/VCF-I-3659 |

---

## Output

After a run, you get:

**CSV Report** (`vcf_votes_report.csv`):
```csv
fr_id,url,status,message,timestamp
VCF-I-979,https://vcf.ideas.aha.io/ideas/VCF-I-979,voted,vote clicked,2026-03-25T10:15:30Z
VCF-I-3704,https://vcf.ideas.aha.io/ideas/VCF-I-3704,already_voted,already voted,2026-03-25T10:15:35Z
```

**Terminal Summary**:
```
## VCF Vote Summary

| Metric       | Count |
|--------------|-------|
| Total FRs    | 5     |
| Voted        | 3     |
| Already voted| 1     |
| Dry run OK   | 0     |
| Failed       | 1     |
```

---

## Running Tests

```bash
# All 20 tests
cargo test

# A specific test
cargo test test_valid_fr_ids

# With output
cargo test -- --nocapture
```

---

## How It Works (For the Curious)

```
main.rs
  |
  +---> Async runtime (tokio)
          |
          +---> browser.rs  — launches Chrome via CDP (chromiumoxide)
          +---> voter.rs    — navigates to FR pages, finds vote buttons via JS
          +---> report.rs   — writes CSV + prints summary
```

The async worker manages state through `Arc<Mutex<AppState>>`. Human-in-the-loop pauses work by prompting in the terminal and waiting for the user to press ENTER before proceeding.

Vote buttons are found using JavaScript evaluation — querying `button, a, [role="button"]` elements by text content. The selectors may need updating if the Aha! portal changes its UI.

---

## Selector Updates

The vote button detection in `src/voter.rs` uses placeholder selectors. After your first dry run with authentication, inspect the actual DOM and update:

- `find_vote_button()` — locates the vote control
- `click_vote_button()` — clicks it
- `attempt_proxy_vote()` — opens proxy dialog, fills fields, submits

---

## Disclaimer

This tool assists with **navigation and clicking**. It does not:
- Store, transmit, or process any credentials
- Bypass CAPTCHAs, bot detection, or rate limits
- Perform any action without explicit human confirmation

Use responsibly and in accordance with the portal's Terms of Use.

---

## License

MIT — do whatever you want with it. If you end up building a CLI for *your* team's internal voting chore, we'd love to hear about it.

---

> *"We over-engineered a voting clicker and we're not sorry."*
