use crate::{
    api::{InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    interfaces::{agent::mcp::McpServer, web},
    net::qos::QosRuntime,
};

use super::{CliError, ServiceMcpTransport, files_cli};

pub(super) async fn run_service(
    mcp: ServiceMcpTransport,
    web_enabled: bool,
) -> Result<String, CliError> {
    let mut runtime = RuntimeConfiguration::from_process_environment()
        .await
        .map_err(|error| CliError::RuntimeConfigFailed(error.to_string()))?;
    if mcp == ServiceMcpTransport::StreamableHttp {
        runtime.agent = runtime.agent.clone().with_streamable_http_enabled();
    }
    runtime.observability.initialize();

    let service = RelayKnowledgeService::new(runtime.clone());
    service
        .reconcile_startup_indexes(RequestContext::for_interface(InterfaceKind::Cli))
        .await
        .map_err(|error| CliError::ServiceRunFailed(error.message))?;
    service
        .recover_orphaned_code_index_tasks_on_startup()
        .await
        .map_err(|error| CliError::ServiceRunFailed(error.message))?;
    service
        .start_code_repository_watcher()
        .await
        .map_err(|error| CliError::ServiceRunFailed(error.message))?;
    let (file_index_shutdown, file_index_shutdown_receiver) = tokio::sync::watch::channel(false);
    let file_index_task = if runtime.file_index.enabled {
        Some(tokio::spawn(files_cli::run_file_index_loop(
            service.clone(),
            runtime.file_index.scan_interval,
            file_index_shutdown_receiver,
        )))
    } else {
        None
    };
    let (code_index_shutdown, code_index_shutdown_receiver) = tokio::sync::watch::channel(false);
    let code_index_tasks = run_code_index_worker_pool(
        service.clone(),
        runtime.workers.code_index_max_in_flight,
        std::time::Duration::from_secs(5),
        code_index_shutdown_receiver,
    );
    eprintln!(
        "relay-knowledge service running; code_index_workers={}",
        code_index_tasks.len()
    );
    let (repo_set_refresh_shutdown, repo_set_refresh_shutdown_receiver) =
        tokio::sync::watch::channel(false);
    let repo_set_refresh_task = tokio::spawn(run_code_repository_set_refresh_loop(
        service.clone(),
        std::time::Duration::from_secs(5),
        repo_set_refresh_shutdown_receiver,
    ));
    if web_enabled {
        let network_config = runtime.network.current();
        ensure_web_remote_bind_allowed(
            &network_config.http,
            runtime.agent.access_policy.allow_remote_clients,
        )?;
        let mut router = web::router(service.clone(), network_config.http.max_request_body_bytes);
        if runtime.agent.mcp_streamable_http_enabled {
            let mcp_router = McpServer::new(
                service.clone(),
                runtime.network.clone(),
                runtime.agent.clone(),
            )
            .checked_router()
            .map_err(|error| CliError::ServiceRunFailed(error.to_string()))?;
            router = router.merge(mcp_router);
        }
        crate::net::http::serve_router_with_qos(
            router,
            network_config.http,
            QosRuntime::default(),
            network_config.qos,
            service_shutdown_signal(),
        )
        .await
        .map_err(|error| CliError::ServiceRunFailed(error.to_string()))?;
    } else if runtime.agent.mcp_streamable_http_enabled {
        let server = McpServer::new(
            service.clone(),
            runtime.network.clone(),
            runtime.agent.clone(),
        );
        server
            .serve_until_shutdown(service_shutdown_signal())
            .await
            .map_err(|error| CliError::ServiceRunFailed(error.to_string()))?;
    } else {
        service_shutdown_signal().await;
    }
    service.stop_code_repository_watcher().await;
    if let Some(task) = file_index_task {
        let _ = file_index_shutdown.send(true);
        let _ = task.await;
    }
    let _ = code_index_shutdown.send(true);
    for task in code_index_tasks {
        let _ = task.await;
    }
    let _ = repo_set_refresh_shutdown.send(true);
    let _ = repo_set_refresh_task.await;
    runtime.observability.shutdown();

    Ok(String::new())
}

pub(super) fn run_code_index_worker_pool(
    service: RelayKnowledgeService,
    worker_count: usize,
    interval: std::time::Duration,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> Vec<tokio::task::JoinHandle<()>> {
    (0..worker_count.max(1))
        .map(|_| {
            tokio::spawn(run_code_index_loop(
                service.clone(),
                interval,
                shutdown.clone(),
            ))
        })
        .collect()
}

async fn run_code_index_loop(
    service: RelayKnowledgeService,
    interval: std::time::Duration,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        let context = RequestContext::for_interface(InterfaceKind::Cli);
        if let Ok(Some(_)) = service.run_code_index_task_once(None, context).await {
            continue;
        }
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
            _ = tokio::time::sleep(interval) => {}
        }
    }
}

pub(super) async fn run_code_repository_set_refresh_loop(
    service: RelayKnowledgeService,
    interval: std::time::Duration,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        let context = RequestContext::for_interface(InterfaceKind::Cli);
        if let Ok(Some(_)) = service
            .run_code_repository_set_refresh_task_once(None, context)
            .await
        {
            continue;
        }
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
            _ = tokio::time::sleep(interval) => {}
        }
    }
}

pub(super) fn ensure_web_remote_bind_allowed(
    config: &crate::net::http::HttpConfig,
    allow_remote_clients: bool,
) -> Result<(), CliError> {
    if crate::net::http::remote_clients_allowed(config, allow_remote_clients) {
        Ok(())
    } else {
        Err(CliError::ServiceRunFailed(
            "Web remote bind requires allow_remote_clients=true".to_owned(),
        ))
    }
}

async fn service_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = terminate.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
