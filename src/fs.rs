use std::fs;
use std::fs::File;
use std::io::{Error, Write};
use std::path::Path;
use walkdir::WalkDir;

pub fn copy_terraform_files(from: &Path, to: &Path) -> Result<(), Error> {
    let files = WalkDir::new(from)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            // return only .tf files
            e.file_name()
                .to_str()
                .map(|s| s.ends_with(".j2.tf") == false && s.ends_with(".tf"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    fs::create_dir_all(to);
    for file in files {
        fs::copy(
            file.path(),
            format!(
                "{}/{}",
                to.to_str().unwrap(),
                file.file_name().to_str().unwrap()
            ),
        );
    }

    Ok(())
}

pub fn write_rendered_templates(
    rendered_templates: &[RenderedTemplate],
    into: &Path,
) -> Result<(), Error> {
    fs::create_dir_all(into);
    for rt in rendered_templates {
        let mut f = File::create(format!("{}/{}", into.to_str().unwrap(), rt.file_name))?;
        f.write_all(rt.content.as_bytes())?;
    }

    Ok(())
}

pub struct RenderedTemplate<'a> {
    pub file_name: &'a str,
    pub content: String,
}

impl<'a> RenderedTemplate<'a> {
    pub fn new(file_name: &'a str, content: String) -> Self {
        RenderedTemplate { file_name, content }
    }
}
