(function (global) {
  "use strict";

  const displayFieldsZh = Object.freeze({
    task: ["任务标题与摘要", "主标题；不同的任务摘要显示在下一行"],
    activity: ["实时状态", "标题栏右侧的运行阶段与耗时"],
    project: ["项目", "副标题中的项目名称"],
    model: ["模型", "副标题中的模型名称"],
    plan: ["计划进度", "完成步数与进度条"],
    sessionTokens: ["会话累计 Token", "折叠卡用量胶囊"],
    context: ["上下文占用", "当前上下文百分比"],
    cost: ["估算 API 价格", "估算值，不是订阅账单"],
    turnTokens: ["本轮 Token", "最近一轮 Token"],
    inputOutputTokens: ["输入 / 输出 Token", "输入与输出拆分"],
    cacheTokens: ["缓存读取 / 写入 Token", "缓存用量拆分"],
    reasoningTokens: ["推理 Token", "Provider 推理用量"],
    tool: ["当前动作", "语义类别与 Provider 工具名"],
    currentTarget: ["当前文件 / 目标", "仅使用 Provider 明确 path 字段的 basename"],
    permissionMode: ["权限模式", ""],
    subagents: ["运行中的子 Agent", ""],
    environment: ["运行环境", ""],
    recovery: ["恢复状态", ""],
    control: ["托管能力", ""],
    jump: ["打开应用", ""],
    taskFlow: ["任务流程", "展开任务后显示当前 Turn 的结构化计划步骤"],
    workflow: ["工作流", "展开任务后显示当前 Turn 的实时工具活动"],
    titleSource: ["标题来源", ""],
    sessionId: ["ActRealm Session ID", ""],
    providerSessionId: ["Provider Session ID", ""],
    providerTurnId: ["Provider Turn ID", ""],
    lastEventAt: ["最后事件时间", ""],
  });

  const concise = ["project", "task", "model", "activity", "plan", "sessionTokens", "context", "taskFlow", "workflow"];
  const detailed = ["project", "task", "model", "activity", "plan", "sessionTokens", "turnTokens", "inputOutputTokens", "cacheTokens", "reasoningTokens", "cost", "context", "tool", "currentTarget", "subagents", "environment", "recovery", "control", "jump", "taskFlow", "workflow"];
  const presets = Object.freeze({
    concise,
    detailed,
    developer: [...detailed, "permissionMode", "titleSource", "sessionId", "providerSessionId", "providerTurnId", "lastEventAt"],
  });

  const categoryLabels = Object.freeze({
    test: ["测试", "Test"],
    build: ["构建", "Build"],
    version_control: ["版本控制", "Version control"],
    package: ["依赖管理", "Dependency management"],
    network: ["网络访问", "Network access"],
    file_edit: ["文件编辑", "File edit"],
    file_read: ["文件读取", "File read"],
    file_search: ["文件查询", "File search"],
    process: ["后台进程", "Background process"],
    code_execution: ["代码执行", "Code execution"],
    interaction: ["界面交互", "Interface interaction"],
    shell: ["Shell 操作", "Shell operation"],
  });

  function categoryLabel(category, locale = "zh") {
    const labels = categoryLabels[category];
    return labels?.[locale === "en" ? 1 : 0];
  }

  function currentAction(session, locale = "zh") {
    if (session?.currentTool) {
      const category = categoryLabel(session.currentToolCategory, locale);
      return category ? `${category} · ${session.currentTool}` : session.currentTool;
    }
    return session?.activity || (locale === "en" ? "No current activity" : "暂无当前动作");
  }

  function currentTarget(session, capability = "unknown", locale = "zh") {
    if (session?.currentTarget) return session.currentTarget;
    const english = locale === "en";
    if (capability === "unsupported") {
      return english ? "Provider does not supply file targets" : "Provider 不提供文件目标";
    }
    if (capability === "unknown") {
      return english ? "No reliable file information" : "暂无可靠文件信息";
    }
    if (session?.execState === "tool_running") {
      return english ? "The current tool has no file target" : "当前工具没有文件目标";
    }
    return english ? "The current phase has no file target" : "当前阶段没有文件目标";
  }

  function titleSource(source, locale = "zh") {
    const english = locale === "en";
    return ({
      codex_thread_name: english ? "Codex conversation name" : "Codex 会话名称",
      claude_custom_title: english ? "Claude custom title" : "Claude 自定义标题",
      claude_session_title: english ? "Claude session title" : "Claude 会话标题",
      claude_ai_title: english ? "Claude generated title" : "Claude 生成标题",
    })[source] || source || (english ? "Safe task summary / project fallback" : "安全任务摘要 / 项目回退");
  }

  function subagentText(session, capability = "unknown", locale = "zh") {
    const count = Number(session?.activeSubagents || 0);
    if (count > 0) return locale === "en" ? `${count} active` : `${count} 个正在运行`;
    if (capability === "supported") return locale === "en" ? "No active subagents" : "暂无活动子 Agent";
    if (capability === "unsupported") return locale === "en" ? "Provider does not support subagent state" : "Provider 不支持子 Agent 状态";
    return locale === "en" ? "Provider subagent capability is not confirmed" : "Provider 子 Agent 能力尚未确认";
  }

  function usageFact(session, locale = "zh") {
    const english = locale === "en";
    const source = ({
      statusline: english ? "StatusLine" : "StatusLine",
      claude_transcript: english ? "Claude transcript" : "Claude transcript",
      claude_transcript_incremental: english ? "Claude transcript (incremental)" : "Claude transcript（增量）",
      codex_rollout: english ? "Codex local rollout" : "Codex 本机 rollout",
      codex_rollout_incremental: english ? "Codex local rollout (incremental)" : "Codex 本机 rollout（增量）",
    })[session?.usageSource] || (english ? "Provider did not supply usage data" : "Provider 未提供用量数据");
    const quality = ({
      official: english ? "official" : "官方",
      official_local: english ? "verified local record" : "已验证本机记录",
      derived: english ? "complete derived record" : "完整派生",
      partial: english ? "partial coverage" : "部分覆盖",
      suspect: english ? "suspect" : "数据可疑",
    })[session?.usageQuality] || (english ? "availability unknown" : "完整性未知");
    return `${source} · ${quality}`;
  }

  function quotaResetSourceLabel(quota) {
    return ({
      statusline: "官方 StatusLine",
      oauth_usage: "官方 OAuth",
      codex_app_server: "官方 Codex",
      rollout_experimental: "本机 Session 解析",
      local_estimate: "本机预计",
    })[quota?.resetSource] || "Provider 未提供";
  }

  function planEmptyText(capability, running, locale = "zh") {
    const english = locale === "en";
    if (capability === "unsupported") return english ? "Provider does not support plan events" : "Provider 不支持计划事件";
    if (capability === "unknown") return english ? "Provider plan capability is not confirmed" : "Provider 计划能力尚未确认";
    if (running) return english ? "Waiting for a plan event in this turn" : "当前 Turn 尚未收到计划事件";
    return english ? "This turn ended without a plan" : "当前 Turn 已结束，未提供计划";
  }

  function matches(start, terminal) {
    if (start.kind !== "tool.started") return false;
    if (start.toolCallId || terminal.toolCallId) return start.toolCallId === terminal.toolCallId;
    return start.toolName === terminal.toolName && (!start.turnId || !terminal.turnId || start.turnId === terminal.turnId);
  }

  function workflowItems(events) {
    const items = [];
    for (const event of events || []) {
      if (event.kind === "tool.started") {
        items.push({ id: event.eventId, event, startedAt: event.occurredAt, target: event.toolTarget });
        continue;
      }
      if (["tool.completed", "tool.failed"].includes(event.kind)) {
        const index = items.findLastIndex((item) => matches(item.event, event));
        if (index >= 0) {
          const start = items[index];
          items[index] = { id: start.id, event, startedAt: start.startedAt, target: start.target || event.toolTarget };
          continue;
        }
      }
      items.push({ id: event.eventId, event, target: event.toolTarget });
    }
    return items;
  }

  function make(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = String(text);
    return node;
  }

  function workflowTitle(item, locale) {
    const event = item.event;
    if (!event.kind.startsWith("tool.")) {
      const labels = {
        "turn.started": ["开始处理任务", "Turn started"],
        "turn.completed": ["本轮任务已完成", "Turn completed"],
        "turn.interrupted": ["本轮任务已中断", "Turn interrupted"],
        "turn.failed": ["本轮任务运行失败", "Turn failed"],
        "plan.updated": ["任务流程已更新", "Plan updated"],
        "approval.requested": ["请求用户批准", "Approval requested"],
        "approval.resolved": ["批准请求已处理", "Approval resolved"],
        "question.requested": ["等待用户回答", "Waiting for an answer"],
        "elicitation.requested": ["请求用户补充信息", "More information requested"],
        "subagent.started": ["子 Agent 已启动", "Subagent started"],
        "subagent.completed": ["子 Agent 已完成", "Subagent completed"],
      };
      return labels[event.kind]?.[locale === "en" ? 1 : 0] || (locale === "en" ? "Agent activity" : "Agent 活动");
    }
    const category = categoryLabel(event.toolCategory, locale);
    const tool = event.toolName || (locale === "en" ? "Tool" : "工具");
    const target = item.target ? ` · ${item.target}` : "";
    const state = event.kind === "tool.started"
      ? (locale === "en" ? "running" : "执行中")
      : event.kind === "tool.failed"
        ? (locale === "en" ? "failed" : "失败")
        : (locale === "en" ? "succeeded" : "成功");
    return `${category ? `${category} · ` : ""}${tool}${target} · ${state}`;
  }

  function renderPlan(panel, session, capability, locale) {
    panel.append(make("strong", "agent-detail-panel-title", locale === "en" ? "Task flow" : "任务流程"));
    const steps = session.planSteps || [];
    if (!steps.length) {
      panel.append(make("p", "agent-detail-empty", planEmptyText(capability, !["idle", "response_finished", "failed"].includes(session.execState), locale)));
      return;
    }
    const source = steps[0].source === "claude_task" ? "Claude Task" : `${session.provider === "claude" ? "Claude" : "Codex"} · ${locale === "en" ? "structured plan" : "结构化计划"}`;
    panel.append(make("small", "agent-detail-source", source));
    const list = make("ol", "agent-detail-scroll agent-plan-list");
    for (const step of steps) {
      const row = make("li", `agent-plan-step ${step.status || "pending"}`);
      row.append(make("span", "agent-plan-state", step.status === "completed" ? "✓" : step.status === "in_progress" ? "●" : "○"));
      const copy = make("div", "");
      copy.append(make("span", "", step.text));
      if (step.detail) copy.append(make("small", "", step.detail));
      row.append(copy);
      list.append(row);
    }
    panel.append(list);
  }

  function renderWorkflowRows(list, events, locale) {
    list.replaceChildren();
    for (const item of workflowItems(events)) {
      const row = make("div", `agent-workflow-row ${item.event.kind === "tool.failed" || item.event.kind === "turn.failed" ? "failed" : ""}`);
      row.append(make("span", "agent-workflow-copy", workflowTitle(item, locale)));
      if (item.startedAt && item.event.occurredAt >= item.startedAt && item.event.kind !== "tool.started") {
        row.append(make("small", "", `${((item.event.occurredAt - item.startedAt) / 1000).toFixed(1)}s`));
      }
      row.append(make("time", "", new Date(item.event.occurredAt).toLocaleTimeString([], { hour12: false })));
      list.append(row);
    }
  }

  async function renderWorkflow(panel, session, capability, locale, api) {
    panel.append(make("strong", "agent-detail-panel-title", locale === "en" ? "Workflow" : "工作流"));
    const status = make("p", "agent-detail-empty", locale === "en" ? "Loading current turn…" : "正在读取当前 Turn…");
    panel.append(status);
    if (capability === "unsupported") {
      status.textContent = locale === "en" ? "Provider does not support tool workflow" : "Provider 不支持工具工作流";
      return;
    }
    try {
      let page = await api(`/api/v1/sessions/${encodeURIComponent(session.id)}/timeline?latest=true&currentTurn=true&limit=100`);
      let events = page.events || [];
      if (!events.length) {
        status.textContent = !["idle", "response_finished", "failed"].includes(session.execState)
          ? (locale === "en" ? "Waiting for the first tool event in this turn" : "等待当前 Turn 的首个工具事件")
          : (locale === "en" ? "No tool calls in this turn" : "当前 Turn 没有工具调用");
        return;
      }
      status.remove();
      const list = make("div", "agent-detail-scroll agent-workflow-list");
      renderWorkflowRows(list, events, locale);
      if (page.hasMore) {
        const load = make("button", "agent-workflow-earlier", locale === "en" ? "Load earlier events" : "加载更早事件");
        load.type = "button";
        load.addEventListener("click", async (event) => {
          event.stopPropagation();
          const first = events[0]?.ingestSequence;
          if (!first) return;
          load.disabled = true;
          try {
            page = await api(`/api/v1/sessions/${encodeURIComponent(session.id)}/timeline?latest=true&currentTurn=true&limit=100&beforeIngestSequence=${first}`);
            const known = new Set(events.map((item) => item.eventId));
            events = [...(page.events || []).filter((item) => !known.has(item.eventId)), ...events].slice(-1000);
            renderWorkflowRows(list, events, locale);
            if (!page.hasMore || events.length >= 1000) load.remove();
          } finally {
            load.disabled = false;
          }
        });
        panel.append(load);
      }
      panel.append(list);
    } catch (_) {
      status.textContent = locale === "en" ? "Workflow is temporarily unavailable" : "工作流暂时无法读取";
      status.classList.add("failed");
    }
  }

  function factSummary(fact, locale = "zh") {
    if (!fact || Number(fact.schemaVersion) !== 1) {
      return locale === "en"
        ? "No trusted fact metadata"
        : "暂无可信事实元数据";
    }
    const english = locale === "en";
    const source = String(fact.sourceId || "");
    const sourceLabel = source.startsWith("connector:")
      ? (english ? "Provider connector" : "Provider Connector")
      : source.startsWith("hook:")
        ? (english ? "Provider hook" : "Provider Hook")
        : source.startsWith("provider:")
          ? (english ? "Provider event" : "Provider 事件")
          : source.startsWith("runtime:")
            ? "Runtime"
            : (english ? "No source" : "无可用来源");
    const freshness = ({
      live: english ? "Live" : "实时",
      delayed: english ? "Delayed" : "延迟",
      stale: english ? "Stale" : "已过期",
      expired: english ? "Expired" : "已失效",
    })[fact.freshness] || (english ? "Stale" : "已过期");
    const verification = ({
      verified: english ? "Verified" : "已验证",
      partial: english ? "Partial" : "部分验证",
      unverified: english ? "Unverified" : "无法验证",
      not_applicable: english ? "Not applicable" : "不适用",
    })[fact.verification] || (english ? "Unverified" : "无法验证");
    const absence = ({
      provider_not_supplied: english ? "Provider did not supply it" : "Provider 未提供",
      not_supported: english ? "Provider does not support it" : "Provider 不支持",
      capability_unconfirmed: english ? "Capability is unconfirmed" : "能力尚未确认",
      no_current_turn: english ? "No current turn" : "当前 Turn 已结束",
      no_current_activity: english ? "No current activity" : "暂无当前活动",
      no_current_tool: english ? "No current tool" : "当前阶段没有工具",
      current_tool_has_no_target: english ? "Current tool has no file target" : "当前工具没有文件目标",
      task_not_completed: english ? "Task is not completed" : "任务尚未完成",
      source_stale: english ? "Source is stale" : "来源已过期",
    })[fact.absenceReason];
    const control = fact.capability === "direct"
      ? (english ? "Direct action" : "可直接处理")
      : fact.capability === "return_to_provider"
        ? (english ? "Return to Provider" : "返回原应用")
        : undefined;
    const captured = Number(fact.capturedAt) > 0
      ? new Date(Number(fact.capturedAt)).toLocaleTimeString(
        english ? "en-US" : "zh-CN",
        {hour: "2-digit", minute: "2-digit", second: "2-digit"},
      )
      : undefined;
    return [sourceLabel, freshness, verification, absence, control, captured]
      .filter(Boolean)
      .join(" · ");
  }

  async function renderReview(panel, session, locale, api) {
    const english = locale === "en";
    panel.append(make("strong", "agent-detail-panel-title", "ActRealm Review"));
    if (session.facts) {
      const evidence = make("div", "agent-review-facts");
      for (const [labelZh, labelEn, fact] of [
        ["计划", "Plan", session.facts.plan],
        ["活动", "Activity", session.facts.activity],
        ["目标", "Target", session.facts.currentTarget],
        ["完成", "Completion", session.facts.completion],
        ["控制", "Control", session.facts.control],
      ]) {
        evidence.append(make(
          "div",
          "agent-review-fact",
          `${english ? labelEn : labelZh} · ${factSummary(fact, locale)}`,
        ));
      }
      panel.append(evidence);
    }
    const status = make(
      "p",
      "agent-detail-empty",
      english ? "Reading local evidence…" : "正在读取本机证据…",
    );
    panel.append(status);
    try {
      const review = await api(`/api/v1/sessions/${encodeURIComponent(session.id)}/review`);
      if (Number(review.schemaVersion) !== 1) {
        status.textContent = english ? "Review contract is unsupported" : "Review 合同暂不支持";
        return;
      }
      status.remove();
      const repository = review.repository || {};
      let repositoryText = repository.state === "available"
        ? `${repository.branch || "detached HEAD"} · ${repository.head || "—"} · ${repository.changedFiles || 0} ${english ? "changes" : "个变更"} · +${repository.insertions ?? "—"} / -${repository.deletions ?? "—"}`
        : repository.state === "not_git"
          ? (english ? "The current workspace is not a Git repository" : "当前工作区不是 Git 仓库")
          : (english ? "Local Git status is unavailable" : "无法读取本机 Git 状态");
      if (Number(repository.commitCount) > 0) {
        repositoryText += ` · ${repository.commitCount} ${english ? "commits" : "个 Commit"}`;
      }
      panel.append(make("p", "agent-detail-review-summary", repositoryText));
      let attribution = ({
        no_changes: english ? "The current working tree is clean" : "当前工作区干净",
        exact: english
          ? "Independent clean worktree; changes are linked exactly to this turn"
          : "独立干净 Worktree；改动与当前 Turn 精确关联",
        concurrent_changes: english
          ? "Another task overlapped this turn in the same workspace; changes cannot be attributed to this task"
          : "当前 Turn 期间同一工作区存在其他任务，不能把全部改动归给当前任务",
        current_worktree_unattributed: english
          ? "Current working-tree state only; no turn-start baseline is available"
          : "仅代表当前工作区状态；尚无 Turn 起点基线",
      })[repository.attribution] || (english ? "Change attribution is unavailable" : "改动归因不可用");
      if (repository.attribution === "bounded_window") {
        attribution = ({
          clean_turn_baseline: english
            ? "Bounded changes measured from the start of this turn"
            : "已从当前 Turn 起点建立有界差异",
          baseline_started_dirty: english
            ? "The turn started dirty; only a bounded working-tree diff is available"
            : "Turn 起点已有改动；只能显示有界工作区差异",
          baseline_captured_after_first_tool: english
            ? "The baseline was captured after the first tool event"
            : "Git 基线晚于首个工具事件；不能声明完整归因",
        })[repository.attributionReason] || (english ? "A bounded turn diff is available" : "已建立有界 Turn 差异");
      }
      panel.append(make("p", "agent-detail-empty", attribution));
      if (repository.baselineState === "available" && repository.baselineCapturedAt) {
        panel.append(make(
          "p",
          "agent-detail-empty",
          `${english ? "Turn start" : "Turn 起点"} · ${repository.baselineHead || "—"} · ${new Date(repository.baselineCapturedAt).toLocaleTimeString(english ? "en-US" : "zh-CN")}`,
        ));
      }
      if ((review.limitations || []).includes("repository_selected_by_unique_dirty_worktree")) {
        panel.append(make(
          "p",
          "agent-detail-empty",
          english
            ? "Repository identified as the only changed nested worktree"
            : "仓库由唯一存在改动的子工作区识别",
        ));
      } else if ((review.limitations || []).includes("repository_selected_as_only_nested_git")) {
        panel.append(make(
          "p",
          "agent-detail-empty",
          english ? "Repository identified as the only nested worktree" : "仓库由唯一的子工作区识别",
        ));
      }
      const validations = Array.isArray(review.validations) ? review.validations.slice(-4) : [];
      if (!validations.length) {
        panel.append(make(
          "p",
          "agent-detail-empty",
          english
            ? "No structured test or build result was observed; this does not mean tests passed"
            : "未观察到结构化测试或构建结果；不等于测试已通过",
        ));
      } else {
        const list = make("div", "agent-review-validations");
        for (const validation of validations) {
          const kind = validation.kind === "test"
            ? (english ? "Test" : "测试")
            : (english ? "Build" : "构建");
          const result = ({
            passed: english ? "Passed" : "通过",
            failed: english ? "Failed" : "失败",
            running: english ? "Running" : "运行中",
            unverifiable: english ? "Executed; result unverified" : "已执行；结果无法验证",
          })[validation.state] || (english ? "Unverified" : "无法验证");
          list.append(make(
            "div",
            `agent-review-validation ${validation.state || "unverified"}`,
            `${kind} · ${validation.toolName || (english ? "Tool" : "工具")} · ${result}`,
          ));
        }
        panel.append(list);
      }
      if (review.lastMeaningfulAction) {
        const action = review.lastMeaningfulAction;
        panel.append(make(
          "p",
          "agent-detail-empty",
          `${english ? "Last meaningful action" : "最后有效动作"} · ${action.kind}${action.toolName ? ` · ${action.toolName}` : ""}`,
        ));
      }
      if (repository.state === "available") {
        const diffButton = make(
          "button",
          "agent-review-diff-button",
          english ? "View local Diff" : "查看本机 Diff",
        );
        diffButton.type = "button";
        const diffRoot = make("div", "agent-review-diff");
        diffButton.addEventListener("click", async () => {
          diffButton.disabled = true;
          diffRoot.textContent = english ? "Reading local file list…" : "正在读取本机文件列表…";
          try {
            const diff = await api(`/api/v1/sessions/${encodeURIComponent(session.id)}/review/diff`);
            diffRoot.replaceChildren();
            const files = make("div", "agent-review-diff-files");
            const patch = rawElement("pre", "agent-review-diff-patch", english
              ? "Select a file to view its local patch"
              : "选择文件查看本机 Patch");
            for (const file of diff.files || []) {
              const fileButton = make("button", "agent-review-diff-file", file.path);
              fileButton.type = "button";
              fileButton.addEventListener("click", async () => {
                fileButton.disabled = true;
                try {
                  const selected = await api(`/api/v1/sessions/${encodeURIComponent(session.id)}/review/diff?path=${encodeURIComponent(file.path)}`);
                  patch.textContent = selected.selected?.patch
                    || (selected.limitation === "untracked_patch_not_read"
                      ? (english ? "Untracked content is not read automatically" : "未跟踪文件内容不会被自动读取")
                      : (english ? "Patch is unavailable" : "Patch 暂不可用"));
                } finally {
                  fileButton.disabled = false;
                }
              });
              files.append(fileButton);
            }
            if (!(diff.files || []).length) {
              files.append(make("p", "agent-detail-empty", english
                ? "No file differences to display"
                : "没有可显示的文件差异"));
            }
            diffRoot.append(files, patch);
          } catch (_) {
            diffRoot.textContent = english ? "Diff is temporarily unavailable" : "Diff 暂时无法读取";
          } finally {
            diffButton.disabled = false;
          }
        });
        panel.append(diffButton, diffRoot);
      }
    } catch (_) {
      status.textContent = english ? "Review is temporarily unavailable" : "Review 暂时无法读取";
      status.classList.add("failed");
    }
  }

  async function renderCheckpoints(panel, session, locale, api) {
    const english = locale === "en";
    panel.append(make("strong", "agent-detail-panel-title", english ? "Checkpoint and recovery" : "Checkpoint 与恢复"));
    const actions = make("div", "agent-checkpoint-actions");
    const label = document.createElement("input");
    label.type = "text";
    label.maxLength = 80;
    label.placeholder = english ? "Optional label" : "可选标签";
    const metadata = make("button", "", english ? "Save metadata" : "保存元数据");
    const git = make("button", "", english ? "Create Git snapshot" : "创建 Git 快照");
    metadata.type = git.type = "button";
    actions.append(label, metadata, git);
    const root = make("div", "agent-checkpoint-list");
    const status = make("p", "agent-detail-empty", english ? "Reading checkpoints…" : "正在读取 Checkpoint…");
    panel.append(actions, status, root);

    const refresh = async () => {
      const response = await api(`/api/v1/sessions/${encodeURIComponent(session.id)}/checkpoints`);
      root.replaceChildren();
      const checkpoints = response.checkpoints || [];
      status.textContent = checkpoints.length
        ? (english ? `${checkpoints.length} local checkpoints` : `${checkpoints.length} 个本机 Checkpoint`)
        : (english ? "No checkpoints yet" : "尚未创建 Checkpoint");
      for (const checkpoint of checkpoints) {
        const card = make("article", "agent-checkpoint-card");
        card.append(
          make("strong", "", checkpoint.label || (checkpoint.kind === "git_snapshot"
            ? (english ? "Git snapshot" : "Git 快照")
            : (english ? "Metadata checkpoint" : "元数据 Checkpoint"))),
          make("small", "", `${checkpoint.provider} · ${new Date(checkpoint.createdAt).toLocaleString(english ? "en-US" : "zh-CN")}`),
        );
        const detail = checkpoint.repository?.state === "available"
          ? `${checkpoint.repository.branch || "detached"} · ${checkpoint.repository.head || "—"} · ${checkpoint.repository.changedFiles || 0} ${english ? "changes" : "个改动"}`
          : (english ? "Repository unavailable" : "仓库不可用");
        card.append(make("p", "agent-detail-empty", detail));
        if ((checkpoint.limitations || []).includes("untracked_not_captured")) {
          card.append(make("p", "agent-detail-empty failed", english
            ? "Untracked files are not captured"
            : "未跟踪文件未包含在 Git 快照中"));
        }
        const controls = make("div", "agent-checkpoint-controls");
        const actionButton = (action, title) => {
          const button = make("button", "", title);
          button.type = "button";
          button.addEventListener("click", async () => {
            button.disabled = true;
            try {
              const preflight = await api(`/api/v1/checkpoints/${encodeURIComponent(checkpoint.id)}/preflight?action=${encodeURIComponent(action)}`);
              if (!preflight.allowed) {
                status.textContent = `${english ? "Blocked" : "已阻止"} · ${(preflight.blockers || []).join(", ")}`;
                return;
              }
              if (!global.confirm(english
                ? `Preflight passed. Execute ${title}?`
                : `预检通过，确认${title}？`)) return;
              await api(`/api/v1/checkpoints/${encodeURIComponent(checkpoint.id)}/actions`, {
                method: "POST",
                body: JSON.stringify({action}),
              });
              status.textContent = english ? "Checkpoint action completed" : "Checkpoint 操作已完成";
            } catch (_) {
              status.textContent = english
                ? "Checkpoint action failed; workspace was preserved"
                : "Checkpoint 操作失败；工作区保持原状";
            } finally {
              button.disabled = false;
            }
          });
          controls.append(button);
        };
        if (checkpoint.providerResumeCapability !== "unsupported") {
          actionButton("resume_session", english ? "Resume session" : "恢复会话");
        }
        if (checkpoint.repository?.gitSnapshot) {
          actionButton("restore_code", english ? "Restore code" : "恢复代码");
          actionButton("rollback_code", english ? "Roll back code" : "回退代码");
        }
        const remove = make("button", "danger", english ? "Delete" : "删除");
        remove.type = "button";
        remove.addEventListener("click", async () => {
          if (!global.confirm(english
            ? "Delete ActRealm checkpoint metadata?"
            : "删除 ActRealm Checkpoint 元数据？")) return;
          await api(`/api/v1/checkpoints/${encodeURIComponent(checkpoint.id)}`, {method: "DELETE"});
          await refresh();
        });
        controls.append(remove);
        card.append(controls);
        root.append(card);
      }
    };
    const create = async (kind) => {
      if (kind === "git_snapshot" && !global.confirm(english
        ? "Create a stash-like Git object without changing the workspace?"
        : "创建不改变工作区的 stash-like Git 对象？")) return;
      metadata.disabled = git.disabled = true;
      try {
        await api(`/api/v1/sessions/${encodeURIComponent(session.id)}/checkpoints`, {
          method: "POST",
          body: JSON.stringify({kind, label: label.value.trim() || null}),
        });
        label.value = "";
        await refresh();
      } catch (_) {
        status.textContent = english ? "Checkpoint creation failed" : "Checkpoint 创建失败";
      } finally {
        metadata.disabled = git.disabled = false;
      }
    };
    metadata.addEventListener("click", () => void create("metadata"));
    git.addEventListener("click", () => void create("git_snapshot"));
    try { await refresh(); }
    catch (_) { status.textContent = english ? "Checkpoints are unavailable" : "Checkpoint 暂时无法读取"; }
  }

  function renderSections(root, session, fields, options) {
    const panels = make("div", "agent-detail-panels");
    const locale = options.locale || "zh";
    const review = make("section", "agent-detail-panel agent-review-panel");
    panels.append(review);
    void renderReview(review, session, locale, options.api);
    const checkpoints = make("section", "agent-detail-panel agent-checkpoint-panel");
    panels.append(checkpoints);
    void renderCheckpoints(checkpoints, session, locale, options.api);
    if (fields.has("taskFlow")) {
      const plan = make("section", "agent-detail-panel");
      renderPlan(plan, session, options.planCapability || "unknown", locale);
      panels.append(plan);
    }
    if (fields.has("workflow")) {
      const workflow = make("section", "agent-detail-panel");
      panels.append(workflow);
      void renderWorkflow(workflow, session, options.workflowCapability || "unknown", locale, options.api);
    }
    root.append(panels);
  }

  const api = {
    displayFieldsZh,
    presets,
    categoryLabel,
    currentAction,
    currentTarget,
    titleSource,
    subagentText,
    usageFact,
    quotaResetSourceLabel,
    planEmptyText,
    workflowItems,
    factSummary,
    renderReview,
    renderCheckpoints,
    renderSections,
  };
  global.ActRealmAgentDetail = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof window !== "undefined" ? window : globalThis);
