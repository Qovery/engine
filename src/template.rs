use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::io::{Error, ErrorKind, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tera::Error as TeraError;
use tera::{Context, Tera};
use walkdir::WalkDir;

pub fn generate_and_copy_all_files_into_dir<S, P>(
    from_dir: S,
    to_dir: P,
    context: &Context,
) -> Result<(), Error>
where
    S: AsRef<Path> + Copy,
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

pub fn copy_non_template_files<S, P>(from: S, to: P) -> Result<(), Error>
where
    S: AsRef<Path>,
    P: AsRef<Path>,
{
    crate::fs::copy_files(from.as_ref(), to.as_ref(), true)
}

pub fn generate_j2_template_files<P>(
    root_dir: P,
    context: &Context,
) -> Result<Vec<RenderedTemplate>, TeraError>
where
    P: AsRef<Path>,
{
    //TODO: sort on fly context should be implemented to optimize reading
    debug!("context: {:#?}", context);
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
        let j2_path_split = j2_path.split("/").collect::<Vec<_>>();
        let j2_root_path: String = j2_path_split.as_slice()[..j2_path_split.len() - 1].join("/");
        let file_name = j2_file_name.replace(".j2", "");

        let content = tera.render(&j2_path[1..], &context)?;

        results.push(RenderedTemplate::new(j2_root_path, file_name, content));
    }

    Ok(results)
}

pub fn write_rendered_templates(
    rendered_templates: &[RenderedTemplate],
    into: &Path,
) -> Result<(), Error> {
    for rt in rendered_templates {
        let dest = format!("{}/{}", into.to_str().unwrap(), rt.path_and_file_name());

        if dest.contains("/") {
            // create the parent directories
            let s_dest = dest.split("/").collect::<Vec<_>>();
            let dir: String = s_dest.as_slice()[..s_dest.len() - 1].join("/");
            let _ = fs::create_dir_all(dir);
        }

        // remove file if it already exists
        let _ = fs::remove_file(dest.as_str());

        // create an empty file
        let mut f = fs::File::create(&dest)?;

        // write rendered template into the new file
        f.write_all(rt.content.as_bytes())?;

        // perform spcific action based on the extension
        let extension = Path::new(&dest).extension().and_then(OsStr::to_str);
        match extension {
            Some("sh") => set_file_permission(&f, 0o755),
            _ => {}
        }
    }

    Ok(())
}

pub fn set_file_permission(f: &File, mode: u32) {
    let metadata = f.metadata().unwrap();
    let mut permissions = metadata.permissions();
    permissions.set_mode(mode);
    f.set_permissions(permissions).unwrap();
}

pub struct RenderedTemplate {
    pub path: String,
    pub file_name: String,
    pub content: String,
}

impl RenderedTemplate {
    pub fn new(path: String, file_name: String, content: String) -> Self {
        RenderedTemplate {
            path,
            file_name,
            content,
        }
    }

    pub fn path_and_file_name(&self) -> String {
        if self.path.trim().is_empty() || self.path.as_str() == "." {
            self.file_name.clone()
        } else {
            format!("{}/{}", self.path.as_str(), self.file_name.as_str())
        }
    }
}
