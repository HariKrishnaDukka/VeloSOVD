//! HTTP route table.

use crate::{api_error::*, models::*, state::*, zonal_client::*};
use axum::{
    Router,
    routing::{get, post},
};

mod diagnostics;
mod executions;
mod faults;
mod vehicle;

pub(crate) use diagnostics::*;
pub(crate) use executions::*;
pub(crate) use faults::*;
pub(crate) use vehicle::*;

pub(crate) fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/topology", get(get_topology))
        .route("/api/health", get(health))
        .route("/api/ecus", get(list_ecus))
        .route("/api/ecus/{id}", get(ecu_info))
        .route("/api/ecus/{id}/resources", get(ecu_resources))
        .route("/api/ecus/{id}/operations", get(ecu_operations))
        .route(
            "/api/ecus/{id}/operation-descriptions/{operation_id}",
            get(ecu_operation),
        )
        .route("/api/ecus/{id}/data/vehicle-speed", get(vehicle_speed))
        .route(
            "/api/ecus/{id}/diagnostics/faults",
            get(faults).delete(delete_faults),
        )
        .route(
            "/api/ecus/{id}/diagnostics/faults/{fault_code}",
            get(fault_by_code).delete(delete_fault),
        )
        .route(
            "/api/ecus/{id}/diagnostics/operations/clear-faults",
            post(clear_faults),
        )
        .route(
            "/api/ecus/{id}/operations/{operation_id}/executions",
            post(execute_clear_faults).get(list_operation_executions),
        )
        .route(
            "/api/ecus/{id}/operations/{operation_id}/executions/{execution_id}",
            get(get_operation_execution),
        )
        .route(
            "/sovd/v1/{entity_collection}/{id}/operations/{operation_id}/executions",
            post(sovd_start_operation_execution).get(sovd_list_operation_executions),
        )
        .route(
            "/sovd/v1/{entity_collection}/{id}/operations/{operation_id}/executions/{execution_id}",
            get(sovd_get_operation_execution)
                .delete(sovd_delete_operation_execution)
                .put(sovd_put_execution_capability),
        )
        .route(
            "/api/ecus/{id}/diagnostics/requests",
            post(diagnostic_request),
        )
        .with_state(state)
}
