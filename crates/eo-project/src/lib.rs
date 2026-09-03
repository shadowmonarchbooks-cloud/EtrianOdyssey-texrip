//! Validated project-manifest serialization.

use eo_core::{CoreError, ProjectManifest};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("project JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("project validation failed: {0}")]
    Core(#[from] CoreError),
}

pub fn to_pretty_json(project: &ProjectManifest) -> Result<String, ProjectError> {
    project.validate()?;
    Ok(serde_json::to_string_pretty(project)?)
}

pub fn from_json(data: &str) -> Result<ProjectManifest, ProjectError> {
    let project: ProjectManifest = serde_json::from_str(data)?;
    project.validate()?;
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eo_core::{GameId, GameIdentity, GameRegion};

    #[test]
    fn valid_manifest_round_trips() {
        let project = ProjectManifest::new(GameIdentity {
            game_id: GameId::EtrianOdysseyUntold,
            region: GameRegion::NorthAmerica,
            title_id: Some("00040000000EC700".parse().unwrap()),
            product_code: Some("CTR-P-BSK-EUR".to_owned()),
        });
        let json = to_pretty_json(&project).unwrap();
        let loaded = from_json(&json).unwrap();
        assert_eq!(loaded, project);
    }

    #[test]
    fn unsupported_schema_is_rejected_on_load() {
        let json = r#"{
            "schema_version": 999,
            "application_version": "0.20.0",
            "game": {
                "game_id": "etrian_odyssey_untold",
                "region": "north_america",
                "title_id": null,
                "product_code": null
            },
            "assets": [],
            "metadata": {}
        }"#;
        assert!(matches!(from_json(json), Err(ProjectError::Core(_))));
    }
}
