(function (global) {
  "use strict";

  const ATTENTION_RANK = Object.freeze({
    error: 0,
    approval: 1,
    native_approval: 2,
    question: 3,
    completion: 5,
  });

  function taskVersion(session, attention = []) {
    const questions = attention.filter((item) => item.sessionId === session.id && item.kind === "question");
    return Math.max(Number(session.lastEventAt || 0), Number(session.turnStartedAt || 0),
      ...questions.map((item) => Number(item.createdAt || 0)));
  }

  function deletionWatermark(session, now, attention = []) {
    return Math.max(now, taskVersion(session, attention));
  }

  function isTaskDeleted(session, removedAt, attention = []) {
    return Number.isFinite(removedAt) && removedAt > 0 && taskVersion(session, attention) <= removedAt;
  }

  function attentionPriority(item) {
    return ATTENTION_RANK[item?.kind] ?? 4;
  }

  function isAcknowledgedCompletion(item) {
    return item?.kind === "completion" && Boolean(item.reminderAcknowledgedAt);
  }

  function pendingForSession(session, attention) {
    return attention
      .filter((item) => item.sessionId === session.id
        && ["open", "committing", "decision_sent"].includes(item.state));
  }

  function actionableForSession(session, attention) {
    return pendingForSession(session, attention).filter((item) => !isAcknowledgedCompletion(item));
  }

  function sessionPriority(session, attention) {
    const pending = actionableForSession(session, attention);
    if (session.execState === "failed" || pending.some((item) => item.kind === "error")) return 0;
    if (pending.some((item) => item.kind === "approval")) return 1;
    if (pending.some((item) => item.kind === "native_approval")) return 2;
    if (pending.some((item) => item.kind === "question")) return 3;
    if (!["idle", "response_finished", "failed"].includes(session.execState)) return 4;
    if (pending.some((item) => item.kind === "completion")) return 5;
    return 6;
  }

  function compareSessions(left, right, attention, selectedID) {
    const leftPriority = sessionPriority(left, attention);
    const rightPriority = sessionPriority(right, attention);
    if (leftPriority !== rightPriority) return leftPriority - rightPriority;
    if (left.id === selectedID) return -1;
    if (right.id === selectedID) return 1;
    if (leftPriority <= 3) {
      const oldest = (session) => actionableForSession(session, attention)
        .reduce((value, item) => Math.min(value, Number(item.createdAt || Infinity)), Infinity);
      const difference = oldest(left) - oldest(right);
      if (difference) return difference;
    } else if (leftPriority === 4) {
      const difference = Number(right.turnStartedAt || right.activitySince || 0)
        - Number(left.turnStartedAt || left.activitySince || 0);
      if (difference) return difference;
    } else {
      const difference = Number(right.lastEventAt || 0) - Number(left.lastEventAt || 0);
      if (difference) return difference;
    }
    return String(left.id).localeCompare(String(right.id));
  }

  function completionSettings(settings) {
    const requestedMode = settings.completionTaskHideMode;
    const requestedMinutes = Number(settings.completionAutoHideMinutes);
    return {
      mode: ["afterConfirmation", "afterDelay"].includes(requestedMode)
        ? requestedMode : "afterConfirmation",
      minutes: [5, 15, 30, 60].includes(requestedMinutes) ? requestedMinutes : 30,
    };
  }

  function acknowledgedCompletionActivity(locale, timing) {
    return {
      className: "idle",
      marker: "✓",
      text: locale === "en"
        ? `Turn complete · scheduled to hide automatically · ${timing}`
        : `本轮已完成 · 将按设定时间自动隐藏 · ${timing}`,
    };
  }

  const api = {
    deletionWatermark,
    isTaskDeleted,
    attentionPriority,
    isAcknowledgedCompletion,
    compareSessions,
    completionSettings,
    acknowledgedCompletionActivity,
  };
  global.ActRealmAgentState = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof window !== "undefined" ? window : globalThis);
