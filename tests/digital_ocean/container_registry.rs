use test_utilities::utilities::context;
use qovery_engine::container_registry::docr::DOCR;
use test_utilities::digitalocean::digital_ocean_token;
use qovery_engine::build_platform::Image;
use qovery_engine::error::EngineError;

#[test]
fn test_create_delete_do_container_registry(){
    let repoToCreate = DOCR {
        context:  context(),
        registry_name: "test-digital-ocean".to_string(),
        api_key: digital_ocean_token(),
        name: "".to_string(),
        id: "".to_string()
    };
    let image = Image{
        application_id: "".to_string(),
        name: "".to_string(),
        tag: "".to_string(),
        commit_id: "".to_string(),
        registry_url: None
    };
    let result = repoToCreate.create_repository(&image);
    // should be created
    match result {
        Ok(_)=> assert!(true),
        _ => assert!(false),
    }
    // now delete it !
    let del_res = repoToCreate.delete_repository(&image);
    match del_res {
        Ok(_)=> assert!(true),
        _ => assert!(false)
    }
}