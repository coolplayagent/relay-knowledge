use crate::{
    api::{
        AuditQueryApiRequest, ProposalDecisionApiRequest, ProposalListApiRequest, RequestContext,
        ServicePlanRequest, WorkerRunRequest, WorkerStatusRequest,
    },
    application::RelayKnowledgeService,
    domain::{ProposalState, ServiceManagerAction, ServiceOperatorState, WorkerKind},
};

use super::{CliAction, CliError, OutputFormat, ServiceMcpTransport, render_response, value_after};

pub(super) async fn run_operational_action(
    service: &RelayKnowledgeService,
    action: &CliAction,
    context: RequestContext,
    format: OutputFormat,
) -> Result<Option<String>, CliError> {
    let output = match action.clone() {
        CliAction::WorkerStatus { kind } => {
            let response = service
                .worker_status(WorkerStatusRequest { kind }, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "worker.status",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        CliAction::WorkerRunOnce { kind } => {
            let response = service
                .run_worker_once(WorkerRunRequest { kind }, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "worker.run_once",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        CliAction::ProposalList { state, limit } => {
            let response = service
                .list_proposals(ProposalListApiRequest { state, limit }, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "proposal.list",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        CliAction::ProposalShow { proposal_id } => {
            let response = service
                .show_proposal(proposal_id, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "proposal.show",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        CliAction::ProposalAccept {
            proposal_id,
            actor,
            reason,
        } => {
            let response = service
                .accept_proposal(
                    proposal_id,
                    ProposalDecisionApiRequest { actor, reason },
                    context,
                )
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "proposal.accept",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        CliAction::ProposalReject {
            proposal_id,
            actor,
            reason,
        } => {
            let response = service
                .decide_proposal_without_commit(
                    proposal_id,
                    ProposalState::Rejected,
                    ProposalDecisionApiRequest { actor, reason },
                    context,
                )
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "proposal.reject",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        CliAction::ProposalSupersede {
            proposal_id,
            actor,
            reason,
        } => {
            let response = service
                .decide_proposal_without_commit(
                    proposal_id,
                    ProposalState::Superseded,
                    ProposalDecisionApiRequest { actor, reason },
                    context,
                )
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "proposal.supersede",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        CliAction::AuditQuery { operation, limit } => {
            let response = service
                .query_audit(AuditQueryApiRequest { operation, limit }, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response("audit.query", response.metadata.clone(), &response, format)?
        }
        CliAction::ServiceStatus => {
            let response = service
                .service_status(context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "service.status",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        CliAction::ServicePlan { action } => {
            let response = service
                .service_plan(ServicePlanRequest { action }, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response("service.plan", response.metadata.clone(), &response, format)?
        }
        CliAction::ServiceDefinitionWrite => {
            let response = service
                .write_service_definition(context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "service.definition.write",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        CliAction::ServiceOperatorStatus => {
            let response = service
                .service_operator_status(context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "service.operator.status",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        CliAction::ServiceOperatorPause => {
            let response = service
                .set_service_operator_state(ServiceOperatorState::Paused, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "service.operator.pause",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        CliAction::ServiceOperatorResume => {
            let response = service
                .set_service_operator_state(ServiceOperatorState::Enabled, context)
                .await
                .map_err(|error| CliError::ApiFailed(error.message))?;

            render_response(
                "service.operator.resume",
                response.metadata.clone(),
                &response,
                format,
            )?
        }
        _ => return Ok(None),
    };

    Ok(Some(output))
}

pub(super) fn parse_worker(tokens: &[String]) -> Result<CliAction, CliError> {
    let action = tokens
        .first()
        .map(String::as_str)
        .ok_or_else(|| CliError::UnexpectedArgument("worker".to_owned()))?;
    let mut kind = None;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--kind" => {
                kind = Some(parse_worker_kind(&value_after(tokens, index, "--kind")?)?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    match action {
        "status" => Ok(CliAction::WorkerStatus { kind }),
        "run-once" => Ok(CliAction::WorkerRunOnce { kind }),
        other => Err(CliError::UnexpectedArgument(other.to_owned())),
    }
}

pub(super) fn parse_proposal(tokens: &[String]) -> Result<CliAction, CliError> {
    match tokens.first().map(String::as_str) {
        Some("list") => parse_proposal_list(&tokens[1..]),
        Some("show") => Ok(CliAction::ProposalShow {
            proposal_id: tokens
                .get(1)
                .cloned()
                .ok_or(CliError::MissingValue("proposal_id"))?,
        }),
        Some("accept") => parse_proposal_decision(&tokens[1..], ProposalDecisionKind::Accept),
        Some("reject") => parse_proposal_decision(&tokens[1..], ProposalDecisionKind::Reject),
        Some("supersede") => parse_proposal_decision(&tokens[1..], ProposalDecisionKind::Supersede),
        Some(other) => Err(CliError::UnexpectedArgument(other.to_owned())),
        None => Err(CliError::UnexpectedArgument("proposal".to_owned())),
    }
}

pub(super) fn parse_audit(tokens: &[String]) -> Result<CliAction, CliError> {
    if tokens.first().map(String::as_str) != Some("query") {
        return Err(CliError::UnexpectedArgument(
            tokens
                .first()
                .cloned()
                .unwrap_or_else(|| "audit".to_owned()),
        ));
    }
    let mut operation = None;
    let mut limit = 100;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--operation" => {
                operation = Some(value_after(tokens, index, "--operation")?);
                index += 2;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::AuditQuery { operation, limit })
}

pub(super) fn parse_service(tokens: &[String]) -> Result<CliAction, CliError> {
    if tokens == ["status"] || tokens == ["doctor"] {
        return Ok(CliAction::ServiceStatus);
    }
    if tokens.first().map(String::as_str) == Some("plan") {
        let value = tokens
            .get(1)
            .ok_or(CliError::MissingValue("install|uninstall"))?;
        return Ok(CliAction::ServicePlan {
            action: parse_service_action(value)?,
        });
    }
    if tokens == ["definition", "write"] {
        return Ok(CliAction::ServiceDefinitionWrite);
    }
    if tokens.first().map(String::as_str) == Some("operator") {
        return match tokens.get(1).map(String::as_str) {
            Some("status") => Ok(CliAction::ServiceOperatorStatus),
            Some("pause") => Ok(CliAction::ServiceOperatorPause),
            Some("resume") => Ok(CliAction::ServiceOperatorResume),
            Some(other) => Err(CliError::UnexpectedArgument(other.to_owned())),
            None => Err(CliError::MissingValue("status|pause|resume")),
        };
    }
    if tokens.first().map(String::as_str) == Some("run") {
        return parse_service_run(&tokens[1..]);
    }

    Err(CliError::UnexpectedArgument(
        tokens
            .first()
            .cloned()
            .unwrap_or_else(|| "service".to_owned()),
    ))
}

fn parse_proposal_list(tokens: &[String]) -> Result<CliAction, CliError> {
    let mut state = None;
    let mut limit = 50;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--state" => {
                state = Some(parse_proposal_state(&value_after(
                    tokens, index, "--state",
                )?)?);
                index += 2;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::ProposalList { state, limit })
}

#[derive(Clone, Copy)]
enum ProposalDecisionKind {
    Accept,
    Reject,
    Supersede,
}

fn parse_proposal_decision(
    tokens: &[String],
    kind: ProposalDecisionKind,
) -> Result<CliAction, CliError> {
    let proposal_id = tokens
        .first()
        .cloned()
        .ok_or(CliError::MissingValue("proposal_id"))?;
    let mut actor = None;
    let mut reason = None;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--by" => {
                actor = Some(value_after(tokens, index, "--by")?);
                index += 2;
            }
            "--reason" => {
                reason = Some(value_after(tokens, index, "--reason")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }
    let actor = actor.ok_or(CliError::MissingValue("--by"))?;

    Ok(match kind {
        ProposalDecisionKind::Accept => CliAction::ProposalAccept {
            proposal_id,
            actor,
            reason,
        },
        ProposalDecisionKind::Reject => CliAction::ProposalReject {
            proposal_id,
            actor,
            reason,
        },
        ProposalDecisionKind::Supersede => CliAction::ProposalSupersede {
            proposal_id,
            actor,
            reason,
        },
    })
}

fn parse_service_run(tokens: &[String]) -> Result<CliAction, CliError> {
    let mut mcp = ServiceMcpTransport::Configured;
    let mut web = false;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--web" => {
                web = true;
                index += 1;
            }
            "--mcp" => {
                let value = value_after(tokens, index, "--mcp")?;
                mcp = match value.as_str() {
                    "streamable-http" => ServiceMcpTransport::StreamableHttp,
                    other => return Err(CliError::UnexpectedArgument(other.to_owned())),
                };
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::ServiceRun { mcp, web })
}

fn parse_worker_kind(value: &str) -> Result<WorkerKind, CliError> {
    WorkerKind::parse(value).map_err(|_| CliError::InvalidWorkerKind(value.to_owned()))
}

fn parse_proposal_state(value: &str) -> Result<ProposalState, CliError> {
    ProposalState::parse(value).map_err(|_| CliError::InvalidProposalState(value.to_owned()))
}

fn parse_service_action(value: &str) -> Result<ServiceManagerAction, CliError> {
    ServiceManagerAction::parse(value).map_err(|_| CliError::InvalidServiceAction(value.to_owned()))
}
