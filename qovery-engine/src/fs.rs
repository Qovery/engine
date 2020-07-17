use std::borrow::Borrow;
use std::fs;
use std::fs::{create_dir_all, File};
use std::io::{Error, ErrorKind, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tera::Error as TeraError;
use tera::{Context, Tera};
use walkdir::WalkDir;

pub fn copy_files(from: &Path, to: &Path, exclude_j2_files: bool) -> Result<(), Error> {
    let files = WalkDir::new(from)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok());

    let files = match exclude_j2_files {
        true => files
            .filter(|e| {
                // return only non *.j2.* files
                e.file_name()
                    .to_str()
                    .map(|s| !s.contains(".j2."))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>(),
        false => files.collect::<Vec<_>>(),
    };

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

pub fn copy_non_template_files<P>(from: P, to: P) -> Result<(), Error>
where
    P: AsRef<Path>,
{
    copy_files(from.as_ref(), to.as_ref(), true)
}

pub fn generate_j2_template_files<P>(
    root_dir: P,
    context: &Context,
) -> Result<Vec<RenderedTemplate>, TeraError>
where
    P: AsRef<Path>,
{
    let root_dir_str = root_dir.as_ref().to_str().unwrap();
    let tera_template_string = format!("{}/**/*.j2.*", root_dir_str);

    let tera = Tera::new(tera_template_string.as_str())?;

    let files = WalkDir::new(root_dir_str)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|s| s.contains(".j2."))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    let mut results: Vec<RenderedTemplate> = vec![];

    for file in files.into_iter() {
        let path_str = file.path().to_str().unwrap();
        let j2_path = path_str.replace(root_dir_str, "");

        let j2_file_name = file.file_name().to_str().unwrap();
        let file_name = j2_file_name.replace(".j2", "");

        let content = tera.render(&j2_path[1..], &context)?;
        results.push(RenderedTemplate::new(file_name, content));
    }

    Ok(results)
}

pub fn generate_and_copy_all_files_into_dir<P>(
    from_dir: P,
    to_dir: P,
    context: &Context,
) -> Result<(), Error>
where
    P: AsRef<Path> + Copy,
{
    // generate j2 templates
    let rendered_templates = match generate_j2_template_files(from_dir, context) {
        Ok(rt) => rt,
        Err(e) => {
            let error_msg = match e.kind {
                tera::ErrorKind::TemplateNotFound(x) => format!("template not found: {}", x),
                tera::ErrorKind::Msg(x) => format!("tera error: {}", x),
                tera::ErrorKind::CircularExtend {
                    tpl,
                    inheritance_chain,
                } => format!(
                    "circular extend - template: {}, inheritance chain: {:?}",
                    tpl, inheritance_chain
                ),
                tera::ErrorKind::MissingParent { current, parent } => {
                    format!("missing parent - current: {}, parent: {}", current, parent)
                }
                tera::ErrorKind::FilterNotFound(x) => format!("filter not found: {}", x),
                tera::ErrorKind::TestNotFound(x) => format!("test not found: {}", x),
                tera::ErrorKind::InvalidMacroDefinition(x) => {
                    format!("invalid macro definition: {}", x)
                }
                tera::ErrorKind::FunctionNotFound(x) => format!("function not found: {}", x),
                tera::ErrorKind::Json(x) => format!("json error: {:?}", x),
                tera::ErrorKind::CallFunction(x) => format!("call function: {}", x),
                tera::ErrorKind::CallFilter(x) => format!("call filter: {}", x),
                tera::ErrorKind::CallTest(x) => format!("call test: {}", x),
                tera::ErrorKind::__Nonexhaustive => format!("non exhaustive error"),
            };

            error!("{}", error_msg.as_str());
            return Err(Error::new(ErrorKind::InvalidData, error_msg));
        }
    };

    // copy all .tf and .yaml files into our dest directory
    copy_non_template_files(from_dir.as_ref(), to_dir.as_ref())?;

    write_rendered_templates(&rendered_templates, to_dir.as_ref())?;

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

pub fn workspace_directory<S, P>(execution_id: S, dir_name: P) -> String
where
    S: AsRef<Path>,
    P: AsRef<Path>,
{
    let dir = format!(
        ".qovery-workspace/{}/{}-{}",
        execution_id.as_ref().to_str().unwrap(),
        dir_name.as_ref().to_str().unwrap(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_millis()
    );

    let _ = create_dir_all(&dir);

    dir
}
