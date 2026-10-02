import Foundation

/// User-facing actions derived only from Runtime categories and known tool
/// identities. Never guesses a filename, command argument or successful result.
public enum TaskActivityPresentation {
    public static func category(_ category: String?, tool: String?) -> String? {
        let name = tool?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() ?? ""
        if name.contains("cua_repl") || name.contains("computer_use") { return "interaction" }
        if name.contains("node_repl") || name == "exec" { return "code_execution" }
        if let category, !["tool", "shell"].contains(category) { return category }
        switch name {
        case "read", "view_image": return "file_read"
        case "edit", "write", "multiedit", "apply_patch": return "file_edit"
        case "grep", "glob", "find", "search": return "file_search"
        case "websearch", "webfetch", "web_search", "web_fetch": return "network"
        case "bash", "shell", "exec_command": return "shell"
        case "write_stdin", "wait", "wait_agent": return "process"
        default: return category
        }
    }

    public static func action(category rawCategory: String?, tool: String?, running: Bool, language: AppLanguage) -> String {
        let name = tool?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let key: String
        if name.lowercased() == "view_image" {
            key = running ? "正在查看图像" : "查看图像"
        } else {
            switch category(rawCategory, tool: tool) {
            case "test": key = running ? "正在运行测试" : "运行测试"
            case "build": key = running ? "正在构建项目" : "构建项目"
            case "code_check": key = running ? "正在检查代码" : "检查代码"
            case "version_control": key = running ? "正在处理版本控制" : "版本控制操作"
            case "package": key = running ? "正在处理依赖" : "处理依赖"
            case "network": key = running ? "正在访问网络" : "访问网络"
            case "file_edit": key = running ? "正在编辑文件" : "编辑文件"
            case "file_read": key = running ? "正在读取文件" : "读取文件"
            case "file_search": key = running ? "正在查询项目" : "查询项目"
            case "process": key = running ? "正在处理后台进程" : "处理后台进程"
            case "code_execution": key = running ? "正在执行代码" : "执行代码"
            case "interaction": key = running ? "正在操作界面" : "操作界面"
            case "shell": key = running ? "正在执行命令" : "执行命令"
            default:
                if !name.isEmpty && name.lowercased() != "unknown" {
                    return AppLocalization.formatted(running ? "正在调用 %@" : "调用 %@", name, language: language)
                }
                key = running ? "正在执行工具" : "执行工具"
            }
        }
        return AppLocalization.localized(key, language: language)
    }
}
