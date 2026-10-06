use super::Application;
use crate::{
    ApplicationError,
    room_assets::{AcquiredModel, Advertisement, ModelTransfers},
};
use bongocat_config::ModelInputMode;
use bongocat_model::{ModelId, ModelOrigin};
use bongocat_model_store::MverInputMode;

impl Application {
    pub(crate) fn refresh_room_cache(&self, transfers: &ModelTransfers) {
        transfers.set_cache(
            self.config
                .model
                .imported_models
                .iter()
                .filter_map(|record| {
                    self.model_directory(ModelOrigin::Installed, &record.id)?;
                    Some((
                        Advertisement {
                            id: record.id.clone(),
                            title: record.title.clone(),
                            input_mode: record.input_mode,
                            library_url: record.library_url.clone(),
                            cache_id: record.shared_model_id.clone(),
                        },
                        record.id.clone(),
                    ))
                }),
        );
    }
    pub(crate) fn prepare_room_share(&self, transfers: &ModelTransfers) {
        self.refresh_room_cache(transfers);
        let snapshot = self.runtime_client().snapshot();
        let share = snapshot
            .active_model
            .as_ref()
            .filter(|_| snapshot.active_model_origin == Some(ModelOrigin::Installed))
            .and_then(|model| {
                self.config
                    .model
                    .imported_models
                    .iter()
                    .find(|record| record.id == model.id.as_str())
            })
            .and_then(|record| {
                self.model_directory(ModelOrigin::Installed, &record.id)
                    .map(|path| {
                        (
                            Advertisement {
                                id: record.id.clone(),
                                title: record.title.clone(),
                                input_mode: record.input_mode,
                                library_url: record.library_url.clone(),
                                cache_id: record.shared_model_id.clone(),
                            },
                            path,
                        )
                    })
            });
        transfers.set_local(share);
    }
    pub(crate) fn import_room_model(
        &mut self,
        item: &AcquiredModel,
        transfers: &ModelTransfers,
    ) -> Result<String, ApplicationError> {
        let selected_mode = match item.model.input_mode {
            ModelInputMode::Standard => MverInputMode::Standard,
            ModelInputMode::Keyboard => MverInputMode::Keyboard,
            ModelInputMode::Gamepad => MverInputMode::Gamepad,
        };
        let models = self.import_models_with_selected_modes_with_observer(
            &item.model.title,
            &item.source,
            vec![selected_mode],
            |_| {},
            || !transfers.current(item),
        )?;
        let id = models
            .first()
            .ok_or_else(|| {
                ApplicationError::ModelNotFound(
                    ModelId::parse(&item.model.id).expect("validated advertisement"),
                )
            })?
            .id()
            .as_str()
            .to_owned();
        let mut records = self.config.model.imported_models.clone();
        if let Some(record) = records.iter_mut().find(|record| record.id == id) {
            record.library_url = item.model.library_url.clone();
            record.shared_model_id = Some(
                item.model
                    .cache_id
                    .clone()
                    .unwrap_or_else(|| item.model.id.clone()),
            );
            self.commit_installed_model_metadata(records)?;
        }
        self.refresh_room_cache(transfers);
        Ok(id)
    }
    pub(crate) fn record_library_source(
        &mut self,
        id: &str,
        url: Option<String>,
    ) -> Result<(), ApplicationError> {
        let mut records = self.config.model.imported_models.clone();
        if let Some(record) = records.iter_mut().find(|record| record.id == id) {
            record.library_url = url;
            self.commit_installed_model_metadata(records)?;
        }
        Ok(())
    }
}
