(() => {
  const root = () => document.querySelector("#token-decision-summary");
  const text = (tag, className, value) => {
    const node = document.createElement(tag);
    node.className = className;
    node.textContent = value;
    return node;
  };
  const tokens = (value) => {
    const number = Number(value || 0);
    if (number >= 1e9) return `${(number / 1e9).toFixed(1)}B`;
    if (number >= 1e6) return `${(number / 1e6).toFixed(1)}M`;
    if (number >= 1e3) return `${(number / 1e3).toFixed(1)}K`;
    return String(number);
  };
  function render(decision, settings) {
    const host = root();
    if (!host) return;
    const showAllocation = settings?.tokenUsageTaskProjectVisible !== false;
    const showBurn = settings?.tokenUsageBurnRateVisible !== false;
    if (!decision || (!showAllocation && !showBurn)) {
      host.hidden = true;
      host.replaceChildren();
      return;
    }
    const fragment = document.createDocumentFragment();
    fragment.append(text("strong", "token-decision-title", "TOKEN 决策"));
    if (showAllocation) {
      const projectCoverage = (Number(
        decision.projectAttributionCoverageBasisPoints
          ?? decision.attributionCoverageBasisPoints
          ?? 0
      ) / 100).toFixed(1);
      const taskCoverage = (Number(
        decision.taskAttributionCoverageBasisPoints
          ?? decision.attributionCoverageBasisPoints
          ?? 0
      ) / 100).toFixed(1);
      const facts = document.createElement("div");
      facts.className = "token-decision-facts";
      facts.append(
        text("span", "", `项目 ${projectCoverage}%`),
        text("span", "", `项目未识别 ${tokens(
          decision.projectUnattributedTokens ?? decision.unattributedTokens
        )}`),
        text("span", "", `任务 ${taskCoverage}%`),
        text("span", "", `任务不可恢复 ${tokens(
          decision.taskUnattributedTokens ?? decision.unattributedTokens
        )}`),
      );
      fragment.append(facts);
      (decision.projectTotals || []).slice(0, 3).forEach((project) => {
        const row = document.createElement("div");
        row.className = "token-decision-row";
        const sessionCount = Number(project.sessionCount || 0);
        row.append(
          text("span", "", sessionCount > 0
            ? `${project.project} · ${sessionCount} sessions`
            : project.project),
          text("strong", "", tokens(project.total))
        );
        fragment.append(row);
      });
    }
    if (showBurn) {
      const burn = (decision.burnRates || [])[0];
      const row = document.createElement("div");
      row.className = `token-burn-row${burn?.state === "elevated" ? " elevated" : ""}`;
      row.append(
        text("span", "", "当前燃烧速度"),
        text("strong", "", burn
          ? burn.state === "collecting" ? "采样中" : `${tokens(burn.tokensPerMinute)} / 分钟`
          : "无运行样本"),
      );
      fragment.append(row);
    }
    fragment.append(text(
      "small",
      "token-decision-source",
      `本机事实账本 · ${decision.freshness || "unavailable"}`,
    ));
    host.replaceChildren(fragment);
    host.hidden = false;
  }
  function settings(state) {
    return {
      tokenUsageTaskProjectVisible: state.tokenUsageTaskProjectVisible !== false,
      tokenUsageBurnRateVisible: state.tokenUsageBurnRateVisible !== false,
      tokenUsageAnomalyVisible: state.tokenUsageAnomalyVisible !== false,
      tokenThresholdNotificationsEnabled: state.tokenThresholdNotificationsEnabled === true,
      tokenThresholdTokensPerMinute: Number(state.tokenThresholdTokensPerMinute || 250000),
    };
  }
  window.TokenDecision = Object.freeze({ render, settings });
})();
