use super::*;

#[derive(Clone, Copy)]
pub(super) enum LostResponse {
    None,
    Answer,
    Variables,
}

pub(super) struct RecoveringApplicationEffects {
    inner: WorkflowApplicationEffectsService,
    lose_answer: AtomicBool,
    lose_variables: AtomicBool,
}

impl RecoveringApplicationEffects {
    pub(super) fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        failure: LostResponse,
    ) -> Self {
        Self {
            inner: WorkflowApplicationEffectsService::new(sessions),
            lose_answer: AtomicBool::new(matches!(failure, LostResponse::Answer)),
            lose_variables: AtomicBool::new(matches!(failure, LostResponse::Variables)),
        }
    }
}

#[async_trait]
impl IWorkflowApplicationEffectsPort for RecoveringApplicationEffects {
    async fn read_conversation_variables(
        &self,
        reference: &WorkflowApplicationRunReference,
    ) -> ApplicationResult<WorkflowApplicationVariableSnapshot> {
        self.inner.read_conversation_variables(reference).await
    }

    async fn append_answer(
        &self,
        request: &WorkflowApplicationMessageRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>> {
        let committed = self.inner.append_answer(request).await?;
        if self.lose_answer.swap(false, Ordering::SeqCst) {
            return Err(ApplicationError::Unavailable(
                "injected lost Answer response after PostgreSQL commit".into(),
            ));
        }
        Ok(committed)
    }

    async fn append_final_output(
        &self,
        request: &WorkflowApplicationMessageRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>> {
        self.inner.append_final_output(request).await
    }

    async fn advance_conversation_variables(
        &self,
        request: &WorkflowApplicationVariableWriteRequest,
    ) -> ApplicationResult<IdempotentWrite<ConversationVariableRevision>> {
        let committed = self.inner.advance_conversation_variables(request).await?;
        if self.lose_variables.swap(false, Ordering::SeqCst) {
            return Err(ApplicationError::Unavailable(
                "injected lost variable response after PostgreSQL commit".into(),
            ));
        }
        Ok(committed)
    }

    async fn observe_terminal(
        &self,
        request: &WorkflowApplicationTerminalRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationInvocation>> {
        self.inner.observe_terminal(request).await
    }
}

struct UnusedExecutionPort;

#[async_trait]
impl IWorkflowExecutionPort for UnusedExecutionPort {
    async fn start_or_adopt(
        &self,
        _request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<Execution> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the Execution port".into(),
        ))
    }

    async fn adopt(
        &self,
        _request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<Option<Execution>> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the Execution port".into(),
        ))
    }

    async fn request_cancellation(
        &self,
        _request: &WorkflowExecutionRequest,
        _requested_at: chrono::DateTime<Utc>,
    ) -> ApplicationResult<Option<Execution>> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the Execution port".into(),
        ))
    }
}

struct UnusedCompositePort;

#[async_trait]
impl IWorkflowCompositeExecutionPort for UnusedCompositePort {
    async fn start_or_adopt(
        &self,
        _request: &WorkflowCompositeExecutionRequest,
    ) -> ApplicationResult<WorkflowRunRecord> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the composite port".into(),
        ))
    }

    async fn adopt(
        &self,
        _request: &WorkflowCompositeExecutionRequest,
    ) -> ApplicationResult<Option<WorkflowRunRecord>> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the composite port".into(),
        ))
    }

    async fn request_cancellation(
        &self,
        _request: &WorkflowCompositeExecutionRequest,
        _reason: Option<String>,
        _requested_by: PrincipalId,
        _requested_at: chrono::DateTime<Utc>,
    ) -> ApplicationResult<Option<WorkflowRunRecord>> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the composite port".into(),
        ))
    }
}

struct UnusedConnectorPort;

#[async_trait]
impl IWorkflowConnectorPort for UnusedConnectorPort {
    async fn execute_attempt(
        &self,
        _request: &WorkflowConnectorAttemptRequest,
    ) -> ApplicationResult<WorkflowConnectorAttemptResult> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the Connector port".into(),
        ))
    }
}

struct UnusedAgentPort;

#[async_trait]
impl IWorkflowAgentPort for UnusedAgentPort {
    async fn start_or_adopt(
        &self,
        _request: &WorkflowAgentRequest,
    ) -> ApplicationResult<AgentExecution> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the Agent port".into(),
        ))
    }

    async fn adopt(
        &self,
        _request: &WorkflowAgentRequest,
    ) -> ApplicationResult<Option<AgentExecution>> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the Agent port".into(),
        ))
    }

    async fn request_cancellation(
        &self,
        _request: &WorkflowAgentRequest,
        _requested_at: chrono::DateTime<Utc>,
    ) -> ApplicationResult<Option<AgentExecution>> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the Agent port".into(),
        ))
    }

    async fn terminal_observation(
        &self,
        _request: &WorkflowAgentRequest,
        _execution: &AgentExecution,
    ) -> ApplicationResult<Option<WorkflowAgentTerminalObservation>> {
        Err(ApplicationError::Internal(
            "Application recovery gate reached the Agent port".into(),
        ))
    }
}

pub(super) fn coordinator(
    engine: FlowEngine,
    effects: Arc<dyn IWorkflowApplicationEffectsPort>,
) -> FlowWorkflowRunCoordinator {
    FlowWorkflowRunCoordinator::with_all_ports_and_application_effects(
        engine,
        Arc::new(UnusedExecutionPort),
        Arc::new(UnusedCompositePort),
        Arc::new(UnusedConnectorPort),
        Arc::new(UnusedAgentPort),
        effects,
    )
}
