import ActRealmKit
import SwiftUI

/// Memory-only renderer for Claude AskUserQuestion/Elicitation and managed
/// Codex requestUserInput. Draft values never enter AppModel, UserDefaults,
/// logs, snapshots, or export data.
struct InteractiveQuestionView: View {
    @EnvironmentObject private var model: AppModel
    let entry: OutboxEntry
    let prompt: InteractivePrompt

    @State private var textValues: [String: String] = [:]
    @State private var selectedValues: [String: Set<String>] = [:]
    @State private var otherValues: [String: String] = [:]
    @State private var validationErrors: [String: String] = [:]
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let message = prompt.message, !message.isEmpty {
                Text(message)
                    .font(.system(size: 11.5))
                    .foregroundStyle(DT.textSecondary)
                    .lineSpacing(3)
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(DT.blueBg.opacity(0.62), in: RoundedRectangle(cornerRadius: 10))
                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(DT.blueBadgeStroke, lineWidth: 1))
            }

            ForEach(prompt.questions) { question in
                questionField(question)
            }

            HStack(spacing: 8) {
                Button("发送回答") { Task { await submit(action: "accept") } }
                    .buttonStyle(ActionButtonStyle(kind: .primary, compact: true))
                    .disabled(busy)
                if prompt.kind == "claude_elicitation" {
                    Button("拒绝提供") { Task { await submit(action: "decline") } }
                        .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
                        .disabled(busy)
                    Button("取消请求") { Task { await submit(action: "cancel") } }
                        .buttonStyle(ActionButtonStyle(kind: .tertiary, compact: true))
                        .disabled(busy)
                }
                if prompt.supportsNative {
                    Button("去 Agent 回答") { Task { await submit(action: "native") } }
                        .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
                        .disabled(busy)
                }
                if busy { ProgressView().controlSize(.small) }
            }
        }
        .padding(.top, 10)
        .disabled(!model.canControlRuntime)
    }

    @ViewBuilder
    private func questionField(_ question: InteractiveQuestion) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack(spacing: 4) {
                Text(question.label.isEmpty ? "问题" : question.label)
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(DT.textPrimary)
                if question.required {
                    Text("必填")
                        .font(.system(size: 8.5, weight: .bold))
                        .foregroundStyle(DT.redText)
                }
            }
            if !question.prompt.isEmpty {
                Text(question.prompt)
                    .font(.system(size: 10.5))
                    .foregroundStyle(DT.textWeak)
            }

            switch question.inputType {
            case "choice":
                VStack(spacing: 6) {
                    ForEach(Array(question.options.enumerated()), id: \.offset) { _, option in
                        choiceRow(question: question, option: option)
                    }
                    if question.allowsOther {
                        TextField(
                            "其他答案（可直接输入）",
                            text: dictionaryBinding($otherValues, key: question.id)
                        )
                        .textFieldStyle(.roundedBorder)
                    }
                }
            case "boolean":
                Picker("", selection: dictionaryBinding($textValues, key: question.id)) {
                    Text("请选择").tag("")
                    Text("是").tag("true")
                    Text("否").tag("false")
                }
                .labelsHidden()
                .pickerStyle(.segmented)
            default:
                if question.isSecret {
                    SecureField("仅在内存中提交", text: dictionaryBinding($textValues, key: question.id))
                        .textFieldStyle(.roundedBorder)
                    Text("仅在内存中提交，不写入数据库、日志或导出。")
                        .font(.system(size: 9))
                        .foregroundStyle(DT.greenText)
                } else {
                    TextField(
                        question.inputType == "number" ? "输入数字" : "输入回答",
                        text: dictionaryBinding($textValues, key: question.id)
                    )
                    .textFieldStyle(.roundedBorder)
                }
            }

            if let validationError = validationErrors[question.id] {
                Label(validationError, systemImage: "exclamationmark.circle.fill")
                    .font(.system(size: 9.5, weight: .semibold))
                    .foregroundStyle(DT.redText)
            }
        }
        .padding(10)
        .background(DT.cardStrong.opacity(0.72), in: RoundedRectangle(cornerRadius: 11))
        .overlay(RoundedRectangle(cornerRadius: 11).strokeBorder(DT.hairline, lineWidth: 1))
    }

    private func choiceRow(
        question: InteractiveQuestion,
        option: InteractiveOption
    ) -> some View {
        let selected = selectedValues[question.id, default: []].contains(option.label)
        return Button {
            var values = selectedValues[question.id, default: []]
            if question.multiSelect {
                if selected { values.remove(option.label) } else { values.insert(option.label) }
            } else {
                values = [option.label]
            }
            selectedValues[question.id] = values
            validationErrors[question.id] = nil
        } label: {
            HStack(spacing: 9) {
                Image(systemName: question.multiSelect
                    ? (selected ? "checkmark.square.fill" : "square")
                    : (selected ? "largecircle.fill.circle" : "circle"))
                    .foregroundStyle(selected ? DT.blue : DT.textWeak)
                VStack(alignment: .leading, spacing: 1) {
                    Text(option.label)
                        .font(.system(size: 10.5, weight: .semibold))
                        .foregroundStyle(DT.textPrimary)
                    if let description = option.description, !description.isEmpty {
                        Text(description)
                            .font(.system(size: 9.5))
                            .foregroundStyle(DT.textWeak)
                    }
                }
                Spacer()
            }
            .padding(.horizontal, 9)
            .padding(.vertical, 7)
            .background(selected ? DT.blueBg : DT.cardMedium, in: RoundedRectangle(cornerRadius: 9))
            .overlay(
                RoundedRectangle(cornerRadius: 9)
                    .strokeBorder(selected ? DT.blueBadgeStroke : DT.hairlineSoft, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }

    private func dictionaryBinding(
        _ dictionary: Binding<[String: String]>,
        key: String
    ) -> Binding<String> {
        Binding(
            get: { dictionary.wrappedValue[key, default: ""] },
            set: {
                dictionary.wrappedValue[key] = $0
                validationErrors[key] = nil
            }
        )
    }

    private func submit(action: String) async {
        guard !busy else { return }
        if action != "accept" {
            busy = true
            _ = await model.submitQuestion(entry, action: action)
            busy = false
            return
        }

        var answers: [String: JSONValue] = [:]
        validationErrors = [:]
        for question in prompt.questions {
            switch question.inputType {
            case "choice":
                var values = Array(selectedValues[question.id, default: []]).sorted()
                let other = otherValues[question.id, default: ""]
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                if !other.isEmpty { values.append(other) }
                if values.isEmpty, question.required {
                    validationErrors[question.id] = "请选择一个答案。"
                    return
                }
                if ["claude_question", "codex_user_input"].contains(prompt.kind) {
                    answers[question.id] = .array(values.map(JSONValue.string))
                } else if let first = values.first {
                    answers[question.id] = .string(first)
                }
            case "boolean":
                let value = textValues[question.id, default: ""]
                if value.isEmpty, question.required {
                    validationErrors[question.id] = "请选择“是”或“否”。"
                    return
                }
                if !value.isEmpty {
                    answers[question.id] = prompt.kind == "codex_user_input"
                        ? .array([.string(value)])
                        : .bool(value == "true")
                }
            default:
                let value = textValues[question.id, default: ""]
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                if value.isEmpty, question.required {
                    validationErrors[question.id] = "请输入回答。"
                    return
                }
                guard !value.isEmpty else { continue }
                let normalized: JSONValue
                if question.inputType == "number" {
                    guard let number = Double(value), number.isFinite else {
                        validationErrors[question.id] = "请输入有效数字。"
                        return
                    }
                    normalized = .number(number)
                } else {
                    normalized = .string(value)
                }
                if prompt.kind == "codex_user_input" {
                    let codexValue: String
                    if case .number(let number) = normalized {
                        codexValue = number.rounded() == number
                            && number >= Double(Int64.min)
                            && number <= Double(Int64.max)
                            ? String(Int64(number))
                            : String(number)
                    } else {
                        codexValue = value
                    }
                    answers[question.id] = .array([.string(codexValue)])
                } else {
                    answers[question.id] = normalized
                }
            }
        }

        busy = true
        _ = await model.submitQuestion(entry, action: "accept", answers: answers)
        busy = false
    }
}
