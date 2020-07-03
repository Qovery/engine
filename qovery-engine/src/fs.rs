use std::fs;
use std::fs::{create_dir_all, File};
use std::io::{Error, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub fn copy_bootstrap_files(from: &Path, to: &Path) -> Result<(), Error> {
    let files = WalkDir::new(from)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            // return only .tf files
            e.file_name()
                .to_str()
                .map(|s| !s.contains(".j2."))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    let _ = fs::create_dir_all(to)?;
    let from_str = from.to_str().unwrap();

    for file in files {
        let path_str = file.path().to_str().unwrap();
        let dest = format!(
            "{}{}",
            to.to_str().unwrap(),
            path_str.replace(from_str, "").as_str()
        );

        if file.metadata().unwrap().is_dir() {
            let _ = fs::create_dir_all(&dest)?;
        }

        let _ = fs::copy(file.path(), dest);
    }

    Ok(())
}

pub fn write_rendered_templates(
    rendered_templates: &[RenderedTemplate],
    into: &Path,
) -> Result<(), Error> {
    let _ = fs::create_dir_all(into);
    for rt in rendered_templates {
        let mut f = File::create(format!("{}/{}", into.to_str().unwrap(), rt.file_name))?;
        f.write_all(rt.content.as_bytes())?;
    }

    Ok(())
}

pub struct RenderedTemplate {
    pub file_name: String,
    pub content: String,
}

impl RenderedTemplate {
    pub fn new(file_name: String, content: String) -> Self {
        RenderedTemplate { file_name, content }
    }
}

pub fn workspace_directory<P>(dir_name: P) -> String
where
    P: AsRef<Path>,
{
    let dir = format!(
        ".qovery-workspace/{}-{}",
        dir_name.as_ref().to_str().unwrap(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_millis()
    );

    let _ = create_dir_all(&dir);

    dir
}
