use rmcp::ServiceExt;
use selenium_mcp::{
    chroma::ChromaHandle, cli, embedding::EmbeddingEngine, project, server::SeleniumMcpServer,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir()?;
    let root = project::resolve_project_root(&cwd)?;
    let paths = project::ensure_layout(&root)?;
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not resolve home directory"))?;
    let driver_cache_dir = home.join(".selenium-mcp").join("drivers");
    let project_id = project::project_id(&paths.root);

    if args.first().map(String::as_str) == Some("test") {
        let tag = find_flag_value(&args, "--tag");
        let flow = find_flag_value(&args, "--flow");
        let json_path = find_flag_value(&args, "--json")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| paths.artifacts_dir.join("report.json"));
        let heal_write_back = args.iter().any(|a| a == "--heal-write-back");
        let update_baselines = args.iter().any(|a| a == "--update-baselines");
        let chroma = ChromaHandle::new(
            home.join(".selenium-mcp").join("chroma"),
            selenium_mcp::chroma::DEFAULT_IDLE_TIMEOUT,
        );
        let embedder = EmbeddingEngine::new(home.join(".selenium-mcp").join("models"));
        let suite = cli::run_suite(cli::RunSuiteOptions {
            flows_dir: &paths.flows_dir,
            driver_cache_dir: &driver_cache_dir,
            chroma: &chroma,
            embedder: &embedder,
            project_id: &project_id,
            tag_filter: tag.as_deref(),
            flow_filter: flow.as_deref(),
            heal_write_back,
            baselines_dir: &paths.baselines_dir,
            artifacts_dir: &paths.artifacts_dir,
            update_baselines,
            profiles_dir: &paths.profiles_dir,
        })
        .await;
        for report in &suite.reports {
            println!(
                "{} {}",
                if report.passed { "PASS" } else { "FAIL" },
                report.flow_name
            );
        }
        if let cli::SuiteOutcome::Infra(reason) = &suite.outcome {
            eprintln!("infrastructure error: {reason}");
        }
        // Written even when Chroma is down or the outcome is Infra -- the
        // report is a local computation with no Chroma dependency
        // (design.md §11.1, FR-011).
        match serde_json::to_string_pretty(&suite) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&json_path, json) {
                    eprintln!("warning: failed to write {}: {e}", json_path.display());
                }
            }
            Err(e) => eprintln!("warning: failed to serialize report.json: {e}"),
        }
        let exit_code = cli::exit_code(&suite.outcome);
        // std::process::exit skips destructors -- without this, a Chroma
        // sidecar spawned during the run would leak past process exit since
        // Child's kill_on_drop(true) never fires (FR-010's idle-shutdown
        // promise doesn't help if the parent never lives long enough to
        // idle out).
        chroma.shutdown().await;
        std::process::exit(exit_code);
    }

    tracing::info!(project_root = %paths.root.display(), "starting selenium-mcp");

    let service = SeleniumMcpServer::new(
        driver_cache_dir,
        paths.artifacts_dir,
        paths.flows_dir,
        project_id,
    )
    .serve(rmcp::transport::stdio())
    .await?;
    service.waiting().await?;
    Ok(())
}

fn find_flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
