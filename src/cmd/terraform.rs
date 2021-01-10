use dirs::home_dir;
use retry::delay::Fixed;
use retry::OperationResult;

use crate::cmd::utilities::exec_with_envs_and_output;
use crate::constants::TF_PLUGIN_CACHE_DIR;
use crate::error::{SimpleError, SimpleErrorKind};

fn terraform_exec_with_init_validate_plan(root_dir: &str) -> Result<(), SimpleError> {
    // terraform init
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        let try_result = terraform_exec(root_dir, vec!["init"]);
        match try_result {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => OperationResult::Err(format!("Command error: {:?}", err)),
        }
    });

    let _ = match result {
        Err(err) => match err {
            retry::Error::Operation {
                error: _,
                total_delay: _,
                tries: _,
            } => Ok(Some(false)),
            retry::Error::Internal(err) => Err(SimpleError::new(SimpleErrorKind::Other, Some(err))),
        },
        Ok(_) => Ok(Some(true)),
    };

    match terraform_exec(root_dir, vec!["validate"]) {
        Err(e) => {
            error!("While trying to Terraform validate the rendered templates");
            return Err(e);
        }
        _ => {
            match terraform_exec(root_dir, vec!["plan", "-out", "tf_plan"]) {
                Err(e) => {
                    error!("While trying to Terraform plan the rendered templates");
                    return Err(e);
                }
                Ok(_) => {}
            };
        }
    };
    Ok(())
}

pub fn terraform_exec_with_init_validate_plan_apply(root_dir: &str, dry_run: bool) -> Result<(), SimpleError> {
    match terraform_exec_with_init_validate_plan(root_dir) {
        Ok(_) => match dry_run {
            true => {
                warn!("dry run flag is true, no terraform apply will happens");
            }
            false => match terraform_exec(root_dir, vec!["apply", "-auto-approve", "tf_plan"]) {
                Ok(_) => {}
                Err(e) => {
                    error!("While trying to Terraform apply the rendered templates");
                    return Err(e);
                }
            },
        },
        Err(e) => return Err(e),
    };
    Ok(())
}

pub fn terraform_exec_with_init_plan_apply_destroy(root_dir: &str) -> Result<(), SimpleError> {
    // terraform init and plan
    // should apply before destroy to be sure destroy will compute on all ressources
    match terraform_exec_with_init_validate_plan_apply(root_dir, false) {
        Ok(_) => {}
        Err(e) => {
            return Err(e);
        }
    }

    // terraform destroy
    match terraform_exec(root_dir, vec!["destroy", "-auto-approve"]) {
        Ok(_) => {}
        Err(e) => {
            error!("While trying to Terraform destroy the rendered templates");
            return Err(e);
        }
    };

    Ok(())
}

pub fn terraform_exec_with_init_plan_destroy(root_dir: &str) -> Result<(), SimpleError> {
    match terraform_exec_with_init_validate_plan(root_dir) {
        Ok(_) => {}
        Err(e) => {
            return Err(e);
        }
    }

    // terraform destroy
    match terraform_exec(root_dir, vec!["destroy", "-auto-approve"]) {
        Ok(_) => {}
        Err(e) => {
            error!("Error While trying to Terraform destroy {:?}", e.message);
            return Err(e);
        }
    };

    Ok(())
}

pub fn terraform_exec(root_dir: &str, args: Vec<&str>) -> Result<(), SimpleError> {
    let home_dir = home_dir().expect("Could not find $HOME");
    let tf_plugin_cache_dir = format!("{}/.terraform.d/plugin-cache", home_dir.to_str().unwrap());

    exec_with_envs_and_output(
        format!("{} terraform", root_dir).as_str(),
        args,
        vec![(TF_PLUGIN_CACHE_DIR, tf_plugin_cache_dir.as_str())],
        |line: Result<String, std::io::Error>| {
            let output = line.unwrap();
            info!("{}", output)
        },
        |line: Result<String, std::io::Error>| {
            let output = line.unwrap();
            error!("{}", output);
        },
    )
}
