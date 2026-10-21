//! Read-only detection. Build output is a snapshot, never a project destination.
use crate::{io, project_folder};
use desegni_core::{Error, Result, identity_source_page};
use serde::Serialize;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebsiteFolder {
    pub path: PathBuf,
    pub framework: Option<String>,
    pub output: Option<PathBuf>,
    pub fresh: bool,
    pub pages: Vec<String>,
    pub build_command: Option<Vec<String>>,
    pub install_command: Option<String>,
}
fn read(path: &Path) -> String {
    let mut text = String::new();
    if let Ok(file) = fs::File::open(path) {
        let _ = file.take(1024 * 1024 + 1).read_to_string(&mut text);
    }
    text
}
fn scan(
    root: &Path,
    dir: &Path,
    count: &mut usize,
    newest: &mut SystemTime,
    pages: &mut Vec<String>,
    output: bool,
    public_output: bool,
) -> Result<()> {
    if dir.strip_prefix(root).map_err(io)?.components().count() > 32 {
        return Err(Error("The website has too many nested folders.".into()));
    }
    for entry in fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        *count += 1;
        if *count > 10_000 {
            return Err(Error(
                "The website exceeds the 10,000-entry import limit.".into(),
            ));
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if (name.starts_with('.') && ![".eleventy.js", ".eleventy.cjs"].contains(&name.as_str()))
            || ["node_modules", "target", "vendor", ".desegnos-original"].contains(&name.as_str())
            || (!output
                && (["dist", "build", "out", "_site"].contains(&name.as_str())
                    || (public_output && name == "public")))
        {
            continue;
        }
        let meta = fs::symlink_metadata(entry.path()).map_err(io)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if !output && entry.path().join(".desegnos-site.json").is_file() {
                continue;
            }
            scan(
                root,
                &entry.path(),
                count,
                newest,
                pages,
                output,
                public_output,
            )?;
        } else if meta.is_file() {
            *newest = (*newest).max(meta.modified().map_err(io)?);
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(io)?
                .to_string_lossy()
                .replace('\\', "/");
            if ["html", "htm", "astro", "md", "mdx"]
                .contains(&relative.rsplit('.').next().unwrap_or(""))
                && identity_source_page(&relative, &read(&entry.path())).is_some()
                && (!output || relative.ends_with(".html") || relative.ends_with(".htm"))
            {
                pages.push(relative);
            }
        }
    }
    Ok(())
}
pub fn detect_website_folder(folder: &Path) -> Result<Option<WebsiteFolder>> {
    let root = folder.canonicalize().map_err(io)?;
    let package: serde_json::Value =
        serde_json::from_str(&read(&root.join("package.json"))).unwrap_or_default();
    let dependencies = format!("{} {}", package["dependencies"], package["devDependencies"]);
    let framework = [
        ("astro", "Astro"),
        ("next", "Next"),
        ("gatsby", "Gatsby"),
        ("@11ty/eleventy", "Eleventy"),
        ("vite", "Vite"),
    ]
    .iter()
    .find(|(key, _)| dependencies.contains(&format!("\"{key}\"")))
    .map(|(_, label)| label.to_string())
    .or_else(|| {
        [
            "hugo.toml",
            "hugo.yaml",
            "hugo.yml",
            "config/_default/hugo.toml",
        ]
        .iter()
        .any(|p| root.join(p).exists())
        .then(|| "Hugo".into())
    })
    .or_else(|| {
        [".eleventy.js", ".eleventy.cjs", "eleventy.config.js"]
            .iter()
            .any(|p| root.join(p).exists())
            .then(|| "Eleventy".into())
    });
    let framework = framework
        .or_else(|| {
            [
                (
                    "Astro",
                    &["astro.config.mjs", "astro.config.ts", "astro.config.js"][..],
                ),
                (
                    "Next",
                    &["next.config.js", "next.config.mjs", "next.config.ts"],
                ),
                (
                    "Vite",
                    &["vite.config.js", "vite.config.ts", "vite.config.mjs"],
                ),
                ("Gatsby", &["gatsby-config.js", "gatsby-config.ts"]),
            ]
            .iter()
            .find(|(_, configs)| configs.iter().any(|p| root.join(p).is_file()))
            .map(|(name, _)| name.to_string())
        })
        .or_else(|| {
            (root.join("content").is_dir()
                && ["config.toml", "config.yaml", "config.yml"]
                    .iter()
                    .any(|p| root.join(p).is_file()))
            .then(|| "Hugo".into())
        });
    let mut source_pages = vec![];
    let mut newest = SystemTime::UNIX_EPOCH;
    scan(
        &root,
        &root,
        &mut 0,
        &mut newest,
        &mut source_pages,
        framework.is_none(),
        matches!(framework.as_deref(), Some("Hugo" | "Gatsby")),
    )?;
    let mut result = WebsiteFolder {
        path: root.clone(),
        framework,
        output: None,
        fresh: false,
        pages: source_pages,
        build_command: None,
        install_command: None,
    };
    result.pages.sort();
    if result.framework.is_none() {
        result.fresh = !result.pages.is_empty();
        return Ok(result.fresh.then_some(result));
    }
    let pm = package["packageManager"]
        .as_str()
        .and_then(|s| s.split('@').next())
        .filter(|s| ["npm", "pnpm", "yarn", "bun"].contains(s))
        .unwrap_or_else(|| {
            if root.join("pnpm-lock.yaml").exists() {
                "pnpm"
            } else if root.join("yarn.lock").exists() {
                "yarn"
            } else if root.join("bun.lockb").exists() || root.join("bun.lock").exists() {
                "bun"
            } else {
                "npm"
            }
        });
    if package["scripts"]["build"].as_str().is_some() {
        result.build_command = Some(vec![pm.into(), "run".into(), "build".into()]);
        if !root.join("node_modules").is_dir() {
            result.install_command = Some(format!("{pm} install"));
        }
    } else if result.framework.as_deref() == Some("Hugo") {
        result.build_command = Some(vec!["hugo".into()]);
    } else if result.framework.as_deref() == Some("Eleventy") {
        result.build_command = Some(vec!["node_modules/.bin/eleventy".into()]);
        if !root.join("node_modules").is_dir() {
            result.install_command = Some(format!("{pm} install"));
        }
    }
    let candidates: &[&str] = if matches!(result.framework.as_deref(), Some("Hugo" | "Gatsby")) {
        &["public", "dist", "build", "out", "_site"]
    } else {
        &["dist", "build", "out", "_site"]
    };
    for candidate in candidates {
        let output = root.join(candidate);
        if !output.is_dir()
            || fs::symlink_metadata(&output)
                .map_err(io)?
                .file_type()
                .is_symlink()
        {
            continue;
        }
        let mut pages = vec![];
        let mut built = SystemTime::UNIX_EPOCH;
        scan(
            &output, &output, &mut 0, &mut built, &mut pages, true, false,
        )?;
        if pages.is_empty() {
            continue;
        }
        // Every output page must be at least as recent as the latest source. A partial build
        // cannot hide stale pages behind one recently touched file.
        let oldest_page = pages
            .iter()
            .filter_map(|p| fs::metadata(output.join(p)).ok()?.modified().ok())
            .min()
            .unwrap_or(built);
        let fresh = oldest_page >= newest;
        if result.output.is_none() || fresh {
            result.output = Some(output);
            result.fresh = fresh;
            result.pages = pages;
            if fresh {
                break;
            }
        }
    }
    result.pages.sort();
    Ok(Some(result))
}

pub fn import_detected_website(folder: &Path, fallback: &Path) -> Result<crate::ProjectFile> {
    let detected = detect_website_folder(folder)?
        .ok_or_else(|| Error("Choose a folder with web pages.".into()))?;
    if detected.framework.is_none() {
        return project_folder::import_website_folder(folder);
    }
    if !detected.fresh {
        return Err(Error("Build site before editing it.".into()));
    }
    let output = detected
        .output
        .as_ref()
        .ok_or_else(|| Error("Build site before editing it.".into()))?;
    let project_folder::WebsiteSnapshot { files, skipped } =
        project_folder::read_website_files(output)?;
    let name = format!(
        "{} (Desegnos)",
        detected
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    );
    let mut project = desegni_core::import_website_files(&name, &files)?;
    project.website.as_mut().expect("imported site").framework = detected.framework;
    project
        .website
        .as_mut()
        .expect("imported site")
        .skipped
        .extend(skipped);
    let parent = fallback;
    // A fallback nested inside the source is never acceptable.
    let mut ancestor = parent;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| Error("Choose an absolute project location.".into()))?;
    }
    if parent.starts_with(&detected.path)
        || ancestor
            .canonicalize()
            .map_err(io)?
            .starts_with(&detected.path)
    {
        return Err(Error(
            "Choose a project location outside the source website.".into(),
        ));
    }
    let path = project_folder::create_project_folder(parent, &name, project.id)?;
    let file = crate::ProjectFile::acquire(&path)?;
    file.save(&project).map_err(|e| Error(e.to_string()))?;
    Ok(file)
}
