//! One explicitly authorized live attempt. No key or token leaves native memory.
use desegni_ai::*;
use desegni_core::*;
use desegni_storage::ProjectFile;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

fn write(
    path: impl AsRef<Path>,
    value: &impl serde::Serialize,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let path = path.as_ref();
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)?
        .write_all(&serde_json::to_vec_pretty(value)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn project_input() -> std::result::Result<Project, Box<dyn std::error::Error>> {
    let mut project = Project::new(
        "Live search check".into(),
        Viewport {
            width: 960,
            height: 720,
        },
        Appearance::Light,
    )?;
    let rect = Rect {
        x: 40.0,
        y: 40.0,
        width: 880.0,
        height: 640.0,
    };
    project = project.apply(
        project.revision,
        Command::AddRegion {
            page_id: project.home_page_id,
            rect,
        },
    )?;
    let id = project.pages[0].regions[0].id;
    project=project.apply(project.revision,Command::UpdateRegion{page_id:project.home_page_id,region_id:id,rect,prompt:"Create one compact feature about a real lighthouse. Use web search to find a real public-domain lighthouse photograph on Wikimedia Commons: a direct HTTPS PNG or JPEG thumbnail below 256 KiB. Download it using data-desegnos-download. Write two short factual sentences based on a reliable public source. Include the photo credit and source link beside the photo. No sample content, placeholders, editor notes or invented facts. Use data-desegnos-found on searched text and the photo, pointing to their public source pages. Keep the requested card within its area and mark its root with this request's data-desegnos-region UUID.".into()})?;
    Ok(project)
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--export-existing") {
        let output = args
            .windows(2)
            .find(|pair| pair[0] == "--output")
            .map(|pair| PathBuf::from(&pair[1]))
            .ok_or("Pass a new --output directory.")?;
        let project = desegni_storage::load_project(Path::new(&pair[1]))?;
        let website = compile_website(&project)?;
        std::fs::create_dir(&output)?;
        for (name, bytes) in website.files {
            let dest = output.join(name);
            std::fs::create_dir_all(dest.parent().ok_or("Missing parent")?)?;
            std::fs::write(dest, bytes)?;
        }
        println!("Offline export complete; no provider was contacted.");
        return Ok(());
    }
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--write-fixture") {
        let project = project_input()?;
        write(
            &pair[1],
            &json!({"project":project,"requirements":project.prepare_requirements()?}),
        )?;
        return Ok(());
    }
    let directory = args
        .windows(2)
        .find(|pair| pair[0] == "--account-directory")
        .map(|pair| PathBuf::from(&pair[1]))
        .ok_or("Pass the existing Desegnos ChatGPT account directory.")?;
    if !directory.join("accounts.json").is_file() {
        return Err("The existing account directory was not found.".into());
    }
    let manager = ChatgptManager::initialize(directory)?;
    let view = manager.view().await;
    let account = view
        .active_account_id
        .and_then(|id| view.accounts.iter().find(|account| account.id == id));
    if args.iter().any(|arg| arg == "--inspect") {
        println!(
            "{}",
            json!({"connected":account.is_some_and(|account|account.connected),"planEnabled":account.is_some_and(|account|account.plan_enabled),"repairRequired":account.is_some_and(|account|account.storage_repair_required),"storageAvailable":view.storage_error.is_none(),"generationRequests":0})
        );
        return Ok(());
    }
    if !args.iter().any(|arg| arg == "--allow-plan-generation") {
        return Err(
            "Explicit --allow-plan-generation is required for one chargeable plan attempt.".into(),
        );
    }
    let account = account
        .filter(|account| {
            account.connected && account.plan_enabled && !account.storage_repair_required
        })
        .ok_or("Connect and authorize a renewable ChatGPT account first.")?;
    let output = args
        .windows(2)
        .find(|pair| pair[0] == "--output")
        .map(|pair| PathBuf::from(&pair[1]))
        .ok_or("Pass a new --output directory.")?;
    // Refuse reuse even after interruption: submitting again must be explicit.
    std::fs::create_dir(&output)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o700))?;
    }
    let cancel = CancellationToken::new();
    let client = OpenAiClient::new()?;
    let models = manager
        .discover_models(account.id, &client, &cancel)
        .await?;
    if !models.models.iter().any(|model| model.id == MODEL_ID) {
        return Err("The configured verified model is unavailable to this plan.".into());
    }
    let key = manager.bearer(account.id, &cancel).await?;
    let project = project_input()?;
    let snapshot = project.prepare_requirements()?;
    write(output.join("input-project.json"), &project)?;
    write(output.join("prepared.json"), &snapshot)?;
    std::fs::write(output.join("REQUIREMENTS.md"), &snapshot.markdown)?;
    write(
        output.join("started.json"),
        &json!({"authentication":"chatgpt","model":MODEL_ID,"submittedAt":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),"maximumAttempts":1}),
    )?;
    let search = AtomicUsize::new(0);
    let citations = AtomicUsize::new(0);
    let start = Instant::now();
    let response = client
        .generate_for_auth(
            AuthenticationMethod::Chatgpt,
            &snapshot,
            project.generation_mode,
            &candidate_schema(),
            &ProviderSettings::default(),
            &key,
            &cancel,
            |progress| {
                search.fetch_max(progress.web_search_calls, Ordering::Relaxed);
                citations.fetch_max(progress.citations, Ordering::Relaxed);
            },
        )
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            write(
                output.join("result.json"),
                &json!({"passed":false,"error":error,"durationSeconds":start.elapsed().as_secs_f64(),"webSearchCalls":search.load(Ordering::Relaxed),"citations":citations.load(Ordering::Relaxed),"generationRequests":1}),
            )?;
            return Err(error.into());
        }
    };
    std::fs::write(output.join("provider-output.json"), &response.text)?;
    let usage = response.usage;
    let prepared = client
        .download_images(response, &snapshot, &project, &cancel)
        .await;
    let mut candidate = GenerationCandidate::from_json(&prepared.text)?;
    candidate.resolve_project_assets(&project)?;
    candidate.validate(&snapshot, &project)?;
    let applied = project.apply_generation(&snapshot, &candidate, &[])?;
    ProjectFile::acquire(&output.join("result.desegnos"))?.save(&applied)?;
    let exported = compile_website(&applied)?;
    let export = output.join("website");
    std::fs::create_dir(&export)?;
    for (name, bytes) in &exported.files {
        let path = export.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)?;
    }
    let placeholder_count = applied.pages[0]
        .content
        .as_ref()
        .ok_or("Missing page")?
        .placeholder_metrics()?
        .total;
    let images = exported
        .files
        .keys()
        .filter(|name| name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg"))
        .count();
    let passed = search.load(Ordering::Relaxed) > 0 && images > 0 && placeholder_count == 0;
    write(
        output.join("result.json"),
        &json!({"passed":passed,"authentication":"chatgpt","model":MODEL_ID,"generationRequests":1,"durationSeconds":start.elapsed().as_secs_f64(),"usage":usage,"webSearchCalls":search.load(Ordering::Relaxed),"citations":citations.load(Ordering::Relaxed),"downloadedImages":images,"placeholdersPerPage":[placeholder_count],"ownerRating":null,"confirmedBilling":null}),
    )?;
    println!(
        "{}",
        json!({"passed":passed,"output":output,"generationRequests":1,"downloadedImages":images,"webSearchCalls":search.load(Ordering::Relaxed),"placeholdersPerPage":[placeholder_count]})
    );
    if !passed {
        return Err(
            "The live check did not meet every criterion; no automatic retry was made.".into(),
        );
    }
    Ok(())
}
