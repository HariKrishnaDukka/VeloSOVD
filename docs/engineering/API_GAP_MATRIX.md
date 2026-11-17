# SOVD API Gap Matrix

**Document status:** Engineering assessment; draft for review  
**Baseline:** GitHub `main` commit `5b3854fa2e0d3c3c377b421fe429692a84e5cd22`  
**Reference package:** `openapi-specification-1.1.0-rc1` (ASAM SOVD API version V1.1.0; OpenAPI 3.1.0)  
**Scope:** Every HTTP path/method operation in `sovd-api.yaml` and `authentication-api.yaml` in the supplied ISO 17978-3 OpenAPI package.  
**Important limitation:** The package states that it is informative and that the normative REST API definition is in the specification. This matrix is a source-level gap assessment against the informative package, not a standards-conformance claim.

## Summary

| Status | Count |
|---|---:|
| implemented and tested | 0 |
| implemented, untested | 0 |
| partial | 14 |
| not implemented | 42 |
| deferred by decision | 38 |
| not yet assessed | 0 |
| **Total** | **94** |

**14 of 94 operations are partial or have an implementation analogue.** No operation is classified as fully implemented against the OpenAPI contract. The remaining operations are either deferred by the demonstrator's current scope or have no matching route. A related custom route is not treated as an implementation of the SOVD path/schema contract.

## Status definitions and evidence rules

- **implemented and tested:** exact path, method, contract and behavior are implemented and supported by a relevant test. No row meets this threshold.
- **implemented, untested:** exact contract is implemented, but no relevant test was found. No row meets this threshold.
- **partial:** a related demonstrator capability exists, but the path, schema, behavior, or a combination differs. The **Partial reason** column identifies the observed difference.
- **not implemented:** no matching SOVD operation or defensible equivalent was found in the inspected router.
- **deferred by decision:** capability family intentionally excluded from this demonstrator's current scope; this is not an accidental omission.
- **not yet assessed:** source evidence was insufficient to classify. No row remains in this category.

Rust route evidence points to `crates/sovd-server/src/main.rs`. Route registration is in `build_router` (around lines 1028–1074 in the reviewed baseline); handler names are given where an analogous handler exists. A dash in the test column means no directly relevant test was identified. Tests listed for partial rows exercise custom routes and/or mocked zonal services; they do not prove OpenAPI schema or ISO conformance.

## Important execution capability PUT distinction

The OpenAPI operation for `PUT /{entity-collection}/{entity-id}/operations/{operation-id}/executions/{execution-id}` declares **202 and default** responses. It does **not** declare 501 in the inspected template. In the Rust implementation, `sovd_put_execution_capability` handles:
- `stop`: requests cooperative stop;
- `status`: returns `204 No Content`;
- `execute`, `freeze`, and `reset`: return `501 Not Implemented`;
- unknown capability: returns `400 Bad Request`.

This is a partial capability implementation with different response/behavior semantics. Returning 204 for `status` does not itself perform a status operation. A cooperative stop request does not prove that a remote ECU operation has stopped.

Other Rust 501 cases observed in the source:
- `sovd_start_operation_execution`: `proximity_response` is rejected as not implemented.
- `sovd_put_execution_capability`: `execute`, `freeze`, and `reset` return not implemented.

These are Rust implementation details; they must not be confused with the four separate 501 response declarations in the OpenAPI template (data-lists POST, individual fault DELETE, triggers POST, cyclic-subscriptions POST).

## Deferred capability families

The following are marked **deferred by decision** because they are outside the current virtual-vehicle demonstrator scope: locks, bulk data, software updates, scripts/script executions, triggers, and cyclic subscriptions. This is a project-scope decision, not a claim that the OpenAPI template marks every operation in these families as optional or unsupported.

## Operation-by-operation assessment

| # | OpenAPI path | Method | Resource family | Status | Partial reason | Rust route (if any) | Rust file and function / evidence | Test name (if any) | Notes |
|---:|---|---|---|---|---|---|---|---|---|
| 1 | `/{any-path}/docs` | GET | Capability description | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 2 | `/{entity-collection}/{entity-id}/data-categories` | GET | Data | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 3 | `/{entity-collection}/{entity-id}/data-groups` | GET | Data | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 4 | `/{entity-collection}/{entity-id}/data` | GET | Data | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 5 | `/{entity-collection}/{entity-id}/data/{data-id}` | GET | Data | partial | path differs; schema differs | /api/ecus/{id}/data/vehicle-speed | crates/sovd-server/src/main.rs: vehicle_speed; build_router | — | Only one custom speed resource is exposed; it is not the generic SOVD data resource contract. |
| 6 | `/{entity-collection}/{entity-id}/data/{data-id}` | PUT | Data | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 7 | `/{entity-collection}/{entity-id}/data-lists` | GET | Data lists | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 8 | `/{entity-collection}/{entity-id}/data-lists` | POST | Data lists | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | The OpenAPI template lists HTTP 501 for this operation; that declaration does not mean the Rust server implements it. |
| 9 | `/{entity-collection}/{entity-id}/data-lists/{data-list-id}` | GET | Data lists | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 10 | `/{entity-collection}/{entity-id}/data-lists/{data-list-id}` | DELETE | Data lists | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 11 | `/{entity-collection}` | GET | Discovery | partial | path differs; schema differs; behaviour differs | GET /api/ecus | crates/sovd-server/src/main.rs: list_ecus; build_router | list_ecus_uses_configured_zonal_http_port | Custom ECU list; not generic entity-collection discovery or SOVD response shape. |
| 12 | `/areas/{area-id}/subareas` | GET | Discovery | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 13 | `/components/{component-id}/subcomponents` | GET | Discovery | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 14 | `/areas/{area-id}/related-components` | GET | Discovery | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 15 | `/components/{component-id}/related-apps` | GET | Discovery | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 16 | `/{entity-collection}/{entity-id}` | GET | Discovery | partial | path differs; schema differs | GET /api/ecus/{id} | crates/sovd-server/src/main.rs: ecu_info; build_router | — | Custom ECU detail response; not generic entity representation. |
| 17 | `/{entity-collection}/{entity-id}/configurations` | GET | Configurations | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 18 | `/{entity-collection}/{entity-id}/configurations/{configuration-id}` | GET | Configurations | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 19 | `/{entity-collection}/{entity-id}/configurations/{configuration-id}` | PUT | Configurations | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 20 | `/{entity-collection}/{entity-id}/operations` | GET | Operations | partial | path differs; schema differs | GET /api/ecus/{id}/operations | crates/sovd-server/src/main.rs: ecu_operations; build_router | — | Returns a demonstrator-specific operation descriptor list. |
| 21 | `/{entity-collection}/{entity-id}/operations/{operation-id}` | GET | Operations | partial | path differs; schema differs | GET /api/ecus/{id}/operation-descriptions/{operation_id} | crates/sovd-server/src/main.rs: ecu_operation; build_router | retrieves_individual_operation_description | Custom operation description route and model. |
| 22 | `/{entity-collection}/{entity-id}/operations/{operation-id}/executions` | POST | Operation executions | partial | path differs; schema differs; behaviour differs | POST /sovd/v1/{entity_collection}/{id}/operations/{operation_id}/executions | crates/sovd-server/src/main.rs: sovd_start_operation_execution; build_router | sovd_post_executes_operation_and_records_execution | Only supported collection/operation and request fields are restricted; parameters rejected; custom execution model/response. |
| 23 | `/{entity-collection}/{entity-id}/operations/{operation-id}/executions` | GET | Operation executions | partial | path differs; schema differs | GET /sovd/v1/{entity_collection}/{id}/operations/{operation_id}/executions | crates/sovd-server/src/main.rs: sovd_list_operation_executions; build_router | operation_execution_routes_filter_by_operation_and_ecu | Custom execution list and filtering; not SOVD representation verified. |
| 24 | `/{entity-collection}/{entity-id}/operations/{operation-id}/executions/{execution-id}` | GET | Operation executions | partial | path differs; schema differs | GET /sovd/v1/{entity_collection}/{id}/operations/{operation_id}/executions/{execution_id} | crates/sovd-server/src/main.rs: sovd_get_operation_execution; build_router | operation_execution_routes_filter_by_operation_and_ecu | Custom execution detail response; mocked/unit route test does not establish schema conformance. |
| 25 | `/{entity-collection}/{entity-id}/operations/{operation-id}/executions/{execution-id}` | DELETE | Operation executions | partial | path differs; behaviour differs | DELETE /sovd/v1/{entity_collection}/{id}/operations/{operation_id}/executions/{execution_id} | crates/sovd-server/src/main.rs: sovd_delete_operation_execution; build_router | — | Requests cooperative stop; does not establish remote ECU operation cancellation. |
| 26 | `/{entity-collection}/{entity-id}/operations/{operation-id}/executions/{execution-id}` | PUT | Operation executions | partial | path differs; schema differs; behaviour differs | PUT /sovd/v1/{entity_collection}/{id}/operations/{operation_id}/executions/{execution_id} | crates/sovd-server/src/main.rs: sovd_put_execution_capability; build_router | — | OpenAPI declares 202/default for capability update. Rust returns 204 for status, 501 for execute/freeze/reset, and supports stop; not the same capability/response contract. OpenAPI response set is 202/default (not 501); Rust capability handling differs as detailed above. |
| 27 | `/{entity-collection}/{entity-id}/modes` | GET | Modes | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 28 | `/{entity-collection}/{entity-id}/modes/{mode-id}` | GET | Modes | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 29 | `/{entity-collection}/{entity-id}/modes/{mode-id}` | PUT | Modes | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 30 | `/{entity-collection}/{entity-id}/logs` | GET | Logs | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 31 | `/{entity-collection}/{entity-id}/logs/entries` | GET | Logs | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 32 | `/{entity-collection}/{entity-id}/logs/config` | PUT | Logs | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 33 | `/{entity-collection}/{entity-id}/logs/config` | GET | Logs | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 34 | `/{entity-collection}/{entity-id}/logs/config` | DELETE | Logs | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred with its parent capability family. |
| 35 | `/{entity-collection}/{entity-id}/locks` | POST | Locks | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: lock lifecycle is outside the current demonstrator scope; no lock manager or route is implemented. |
| 36 | `/{entity-collection}/{entity-id}/locks` | GET | Locks | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: lock lifecycle is outside the current demonstrator scope; no lock manager or route is implemented. |
| 37 | `/{entity-collection}/{entity-id}/locks/{lock-id}` | GET | Locks | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: lock lifecycle is outside the current demonstrator scope; no lock manager or route is implemented. |
| 38 | `/{entity-collection}/{entity-id}/locks/{lock-id}` | PUT | Locks | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: lock lifecycle is outside the current demonstrator scope; no lock manager or route is implemented. |
| 39 | `/{entity-collection}/{entity-id}/locks/{lock-id}` | DELETE | Locks | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: lock lifecycle is outside the current demonstrator scope; no lock manager or route is implemented. |
| 40 | `/{entity-collection}/{entity-id}/bulk-data` | GET | Bulk data | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: bulk transfer/storage is outside the current demonstrator scope; no bulk-data service is implemented. |
| 41 | `/{entity-collection}/{entity-id}/bulk-data/{category}` | GET | Bulk data | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: bulk transfer/storage is outside the current demonstrator scope; no bulk-data service is implemented. |
| 42 | `/{entity-collection}/{entity-id}/bulk-data/{category}` | POST | Bulk data | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: bulk transfer/storage is outside the current demonstrator scope; no bulk-data service is implemented. |
| 43 | `/{entity-collection}/{entity-id}/bulk-data/{category}` | DELETE | Bulk data | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: bulk transfer/storage is outside the current demonstrator scope; no bulk-data service is implemented. |
| 44 | `/{entity-collection}/{entity-id}/bulk-data/{category}/{bulk-data-id}` | GET | Bulk data | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: bulk transfer/storage is outside the current demonstrator scope; no bulk-data service is implemented. |
| 45 | `/{entity-collection}/{entity-id}/bulk-data/{category}/{bulk-data-id}` | DELETE | Bulk data | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: bulk transfer/storage is outside the current demonstrator scope; no bulk-data service is implemented. |
| 46 | `/updates` | GET | Software updates | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: ECU update orchestration and package lifecycle are outside the current demonstrator scope. |
| 47 | `/updates` | POST | Software updates | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: ECU update orchestration and package lifecycle are outside the current demonstrator scope. |
| 48 | `/updates/{update-package-id}` | GET | Software updates | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: ECU update orchestration and package lifecycle are outside the current demonstrator scope. |
| 49 | `/updates/{update-package-id}` | DELETE | Software updates | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: ECU update orchestration and package lifecycle are outside the current demonstrator scope. |
| 50 | `/updates/{update-package-id}/automated` | PUT | Software updates | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: ECU update orchestration and package lifecycle are outside the current demonstrator scope. |
| 51 | `/updates/{update-package-id}/execute` | PUT | Software updates | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: ECU update orchestration and package lifecycle are outside the current demonstrator scope. |
| 52 | `/updates/{update-package-id}/prepare` | PUT | Software updates | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: ECU update orchestration and package lifecycle are outside the current demonstrator scope. |
| 53 | `/updates/{update-package-id}/status` | GET | Software updates | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 54 | `/{entity-collection}/{entity-id}/faults` | GET | Faults | partial | path differs; schema differs | GET /api/ecus/{id}/diagnostics/faults | crates/sovd-server/src/main.rs: faults; build_router | retrieves_individual_fault_from_zonal_server | Custom fault collection and optional active filter. |
| 55 | `/{entity-collection}/{entity-id}/faults` | DELETE | Faults | partial | path differs; behaviour differs | DELETE /api/ecus/{id}/diagnostics/faults | crates/sovd-server/src/main.rs: delete_faults; build_router | retrieves_individual_fault_from_zonal_server | Custom collection deletion; route test uses mocked zonal service. |
| 56 | `/{entity-collection}/{entity-id}/faults/{fault-code}` | GET | Faults | partial | path differs; schema differs | GET /api/ecus/{id}/diagnostics/faults/{fault_code} | crates/sovd-server/src/main.rs: fault_by_code; build_router | retrieves_individual_fault_from_zonal_server | Custom fault item route and DiagnosticFault response model. |
| 57 | `/{entity-collection}/{entity-id}/faults/{fault-code}` | DELETE | Faults | partial | path differs; behaviour differs | DELETE /api/ecus/{id}/diagnostics/faults/{fault_code} | crates/sovd-server/src/main.rs: delete_fault; build_router | retrieves_individual_fault_from_zonal_server | Custom fault deletion path; does not prove matching SOVD error/response semantics. |
| 58 | `/{entity-collection}/{entity-id}/communication-logs` | GET | Communication logs | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 59 | `/{entity-collection}/{entity-id}/communication-logs` | POST | Communication logs | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 60 | `/{entity-collection}/{entity-id}/communication-logs/{communication-log-id}` | GET | Communication logs | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 61 | `/{entity-collection}/{entity-id}/communication-logs/{communication-log-id}` | PUT | Communication logs | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 62 | `/{entity-collection}/{entity-id}/communication-logs/{communication-log-id}` | DELETE | Communication logs | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred with its parent capability family. |
| 63 | `/{entity-collection}/{entity-id}/scripts` | GET | Scripts | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: upload, storage and execution of diagnostic scripts are outside the current demonstrator scope. |
| 64 | `/{entity-collection}/{entity-id}/scripts` | POST | Scripts | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: upload, storage and execution of diagnostic scripts are outside the current demonstrator scope. |
| 65 | `/{entity-collection}/{entity-id}/scripts/{script-id}` | GET | Scripts | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: upload, storage and execution of diagnostic scripts are outside the current demonstrator scope. |
| 66 | `/{entity-collection}/{entity-id}/scripts/{script-id}` | DELETE | Scripts | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: upload, storage and execution of diagnostic scripts are outside the current demonstrator scope. |
| 67 | `/{entity-collection}/{entity-id}/scripts/{script-id}/executions` | POST | Script executions | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: script execution lifecycle is outside the current demonstrator scope. |
| 68 | `/{entity-collection}/{entity-id}/scripts/{script-id}/executions` | GET | Script executions | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: script execution lifecycle is outside the current demonstrator scope. |
| 69 | `/{entity-collection}/{entity-id}/scripts/{script-id}/executions/{execution-id}` | GET | Script executions | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: script execution lifecycle is outside the current demonstrator scope. |
| 70 | `/{entity-collection}/{entity-id}/scripts/{script-id}/executions/{execution-id}` | PUT | Script executions | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: script execution lifecycle is outside the current demonstrator scope. |
| 71 | `/{entity-collection}/{entity-id}/scripts/{script-id}/executions/{execution-id}` | DELETE | Script executions | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: script execution lifecycle is outside the current demonstrator scope. |
| 72 | `/{entity-collection}/{entity-id}/triggers` | POST | Triggers | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: trigger configuration and event delivery are outside the current demonstrator scope. |
| 73 | `/{entity-collection}/{entity-id}/triggers` | GET | Triggers | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: trigger configuration and event delivery are outside the current demonstrator scope. |
| 74 | `/{entity-collection}/{entity-id}/triggers/{id}` | GET | Triggers | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: trigger configuration and event delivery are outside the current demonstrator scope. |
| 75 | `/{entity-collection}/{entity-id}/triggers/{id}` | PUT | Triggers | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: trigger configuration and event delivery are outside the current demonstrator scope. |
| 76 | `/{entity-collection}/{entity-id}/triggers/{id}` | DELETE | Triggers | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: trigger configuration and event delivery are outside the current demonstrator scope. |
| 77 | `/{entity-collection}/{entity-id}/cyclic-subscriptions` | GET | Cyclic subscriptions | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: subscription scheduling and streaming delivery are outside the current demonstrator scope. |
| 78 | `/{entity-collection}/{entity-id}/cyclic-subscriptions` | POST | Cyclic subscriptions | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: subscription scheduling and streaming delivery are outside the current demonstrator scope. |
| 79 | `/{entity-collection}/{entity-id}/cyclic-subscriptions/{id}` | GET | Cyclic subscriptions | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: subscription scheduling and streaming delivery are outside the current demonstrator scope. |
| 80 | `/{entity-collection}/{entity-id}/cyclic-subscriptions/{id}` | PUT | Cyclic subscriptions | deferred by decision | — | — | No implementation; scope decision recorded in this matrix. | — | Deferred: subscription scheduling and streaming delivery are outside the current demonstrator scope. |
| 81 | `/{entity-collection}/{entity-id}/cyclic-subscriptions/{id}` | DELETE | Cyclic subscriptions | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 82 | `/{entity-collection}/{entity-id}/clear-data` | GET | Clear data | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 83 | `/{entity-collection}/{entity-id}/clear-data/cached-data` | PUT | Clear data | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 84 | `/{entity-collection}/{entity-id}/clear-data/learned-data` | PUT | Clear data | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 85 | `/{entity-collection}/{entity-id}/clear-data/client-defined-resources` | PUT | Clear data | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 86 | `/{entity-collection}/{entity-id}/clear-data/status` | GET | Clear data | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 87 | `/{entity-collection}/{entity-id}/status` | GET | Entity status | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 88 | `/{entity-collection}/{entity-id}/status/start` | PUT | Entity status | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 89 | `/{entity-collection}/{entity-id}/status/restart` | PUT | Entity status | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 90 | `/{entity-collection}/{entity-id}/status/force-restart` | PUT | Entity status | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 91 | `/{entity-collection}/{entity-id}/status/shutdown` | PUT | Entity status | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 92 | `/{entity-collection}/{entity-id}/status/force-shutdown` | PUT | Entity status | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 93 | `/authorize` | POST | Authentication | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |
| 94 | `/token` | POST | Authentication | not implemented | — | — | crates/sovd-server/src/main.rs: build_router (no matching route/handler) | — | No matching SOVD path/handler is registered in the Rust router. |

## Test interpretation

Tests named above are evidence only for the custom Rust routes they exercise. In particular, `retrieves_individual_fault_from_zonal_server` uses a mocked zonal service; it verifies route forwarding and status behavior, not the OpenAPI schema. `operation_execution_routes_filter_by_operation_and_ecu` similarly verifies custom execution filtering. No test in this assessment validates all 94 operations against the supplied OpenAPI schemas.

## Source and reproducibility

- OpenAPI source files: `sovd-api.yaml` and `authentication-api.yaml`.
- Package directory/archive label: `openapi-specification-1.1.0-rc1`.
- API metadata version: ASAM SOVD V1.1.0.
- OpenAPI format version: 3.1.0.
- Package SHA-256: `4a3fb8bf99acbeb1d751a0dd349612164d5d957d44d7bb5cd7919d035e454212`.
- Rust baseline reviewed: `5b3854fa2e0d3c3c377b421fe429692a84e5cd22`.

This matrix must be revisited when the API base-path decision is made or when routes, schemas, and tests change. It does not authorize a compliance claim.
