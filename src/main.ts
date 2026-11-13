// Settings window. Pure view: all logic lives in Rust.
// Bundled by esbuild to dist/main.js (referenced from index.html).

import { invoke, listen } from "./tauri";

type AppConfig = {
  interval_secs: number;
  probe_timeout_secs: number;
  probe_endpoints: string[];
  ssids: string[];
  portal: {
    login_url: string;
    method: string;
    fields: { name: string; value: string }[];
    extra_headers: { name: string; value: string }[];
    body_type: "form" | "json";
    success: SuccessCheck[];
    pre_step?: { url: string; extract_regex?: string } | null;
  };
  username: string;
  retry: { max_attempts: number; backoff_base_secs: number; backoff_cap_secs: number };
  notify: boolean;
};

type SuccessCheck =
  | { type: "status2xx" }
  | { type: "status"; value: number }
  | { type: "body_contains"; value: string }
  | { type: "body_not_contains"; value: string }
  | { type: "body_regex"; value: string }
  | { type: "reprobe" };

type StatusPayload = {
  state: "online" | "offline" | "authing" | "backoff" | "gave_up" | "paused";
  ssid: string | null;
  ssid_whitelisted: boolean;
  next_probe_in_secs: number;
  fail_count: number;
};

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
// T is only in the return position, so `$()` cannot infer it from arguments —
// use $input() for anything you need .value/.checked on.
const $input = (id: string) => document.getElementById(id) as HTMLInputElement;
const val = (id: string) => $input(id).value;
const setVal = (id: string, v: string | number) => {
  $input(id).value = String(v);
};
// load() sets checkboxes via `.checked`; setVal is for text/number inputs.

const STATE_LABEL: Record<string, string> = {
  online: "已联网",
  offline: "未联网",
  authing: "正在认证…",
  backoff: "重试中…",
  gave_up: "自动登录失败",
  paused: "已暂停",
};

function parseKv(text: string): { name: string; value: string }[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l && l.includes("="))
    .map((l) => {
      const i = l.indexOf("=");
      return { name: l.slice(0, i).trim(), value: l.slice(i + 1).trim() };
    });
}

function dumpKv(fields: { name: string; value: string }[]): string {
  return fields.map((f) => `${f.name}=${f.value}`).join("\n");
}

function parseChecks(text: string): SuccessCheck[] {
  const out: SuccessCheck[] = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) continue;
    if (line === "reprobe") out.push({ type: "reprobe" });
    else if (line === "status2xx" || line === "2xx") out.push({ type: "status2xx" });
    else if (line.startsWith("status=")) out.push({ type: "status", value: Number(line.slice(7)) });
    else if (line.startsWith("contains=")) out.push({ type: "body_contains", value: line.slice(9) });
    else if (line.startsWith("notcontains=")) out.push({ type: "body_not_contains", value: line.slice(12) });
    else if (line.startsWith("regex=")) out.push({ type: "body_regex", value: line.slice(6) });
  }
  return out;
}

function dumpChecks(checks: SuccessCheck[]): string {
  return checks
    .map((c) => {
      switch (c.type) {
        case "reprobe": return "reprobe";
        case "status2xx": return "status2xx";
        case "status": return `status=${c.value}`;
        case "body_contains": return `contains=${c.value}`;
        case "body_not_contains": return `notcontains=${c.value}`;
        case "body_regex": return `regex=${c.value}`;
      }
    })
    .join("\n");
}

function lines(list: string[]): string {
  return list.join("\n");
}

async function load() {
  const cfg = await invoke<AppConfig>("get_config");
  const hasPw = await invoke<boolean>("has_password", { username: cfg.username });

  setVal("interval", cfg.interval_secs);
  setVal("timeout", cfg.probe_timeout_secs);
  setVal("endpoints", lines(cfg.probe_endpoints));
  setVal("ssids", lines(cfg.ssids));
  setVal("method", cfg.portal.method.toUpperCase());
  setVal("body-type", cfg.portal.body_type);
  setVal("login-url", cfg.portal.login_url);
  setVal("fields", dumpKv(cfg.portal.fields));
  setVal("headers", dumpKv(cfg.portal.extra_headers));
  setVal("success", dumpChecks(cfg.portal.success));
  const pre = cfg.portal.pre_step;
  $input("use-prestep").checked = !!pre;
  $("prestep-box").classList.toggle("hidden", !pre);
  if (pre) {
    setVal("prestep-url", pre.url);
    setVal("prestep-regex", pre.extract_regex ?? "");
  }
  setVal("username", cfg.username);
  setVal("max-attempts", cfg.retry.max_attempts);
  setVal("backoff-base", cfg.retry.backoff_base_secs);
  setVal("backoff-cap", cfg.retry.backoff_cap_secs);
  $input("notify").checked = cfg.notify;
  $("pw-state").textContent = hasPw ? "(已保存在 Windows 凭据管理器)" : "(尚未设置)";
  try {
    const enabled = await invoke<boolean>("plugin:autostart|is_enabled");
    $input("autostart").checked = enabled;
  } catch {
    // autostart permission unavailable; leave unchecked
  }
}

async function save() {
  const msg = $("save-msg");
  msg.textContent = "";
  try {
    const cfg: AppConfig = {
      interval_secs: clampInt(val("interval"), 10, 3600, 60),
      probe_timeout_secs: clampInt(val("timeout"), 1, 30, 5),
      probe_endpoints: val("endpoints").split(/\r?\n/).map((s) => s.trim()).filter(Boolean),
      ssids: val("ssids").split(/\r?\n/).map((s) => s.trim()).filter(Boolean),
      portal: {
        login_url: val("login-url").trim(),
        method: val("method").toUpperCase(),
        fields: parseKv(val("fields")),
        extra_headers: parseKv(val("headers")),
        body_type: val("body-type") === "json" ? "json" : "form",
        success: parseChecks(val("success")),
        // A ticked pre-step without a URL would fail at request time — treat
        // it as "not configured".
        pre_step:
          $input("use-prestep").checked && val("prestep-url").trim()
            ? {
                url: val("prestep-url").trim(),
                ...(val("prestep-regex").trim() ? { extract_regex: val("prestep-regex").trim() } : {}),
              }
            : null,
      },
      username: val("username").trim(),
      retry: {
        max_attempts: clampInt(val("max-attempts"), 1, 20, 5),
        backoff_base_secs: clampInt(val("backoff-base"), 1, 300, 10),
        backoff_cap_secs: clampInt(val("backoff-cap"), 1, 1800, 300),
      },
      notify: $input("notify").checked,
    };
    const pw = val("password");
    const auto = ($input("autostart")).checked;
    await invoke("save_config", {
      config: cfg,
      password: pw ? pw : null,
      autostart: auto,
    });
    msg.textContent = "已保存 ✓";
    setVal("password", "");
    // Refresh pw state hint.
    const hasPw = await invoke<boolean>("has_password", { username: cfg.username });
    $("pw-state").textContent = hasPw ? "(已保存在 Windows 凭据管理器)" : "(尚未设置)";
    setTimeout(() => (msg.textContent = ""), 3000);
  } catch (e: any) {
    msg.textContent = "保存失败: " + (e?.message ?? String(e));
  }
}

function clampInt(v: string, min: number, max: number, fallback: number): number {
  const n = parseInt(v, 10);
  if (Number.isNaN(n)) return fallback;
  return Math.min(max, Math.max(min, n));
}

function updateChip(s: StatusPayload) {
  const chip = $("status-chip");
  chip.textContent = STATE_LABEL[s.state] ?? s.state;
  chip.className = "chip " + s.state;
}

function wire() {
  $("btn-save").addEventListener("click", save);
  $("btn-probe").addEventListener("click", () => invoke("probe_now"));
  $("btn-test-notify").addEventListener("click", () => invoke("test_notification"));
  $("btn-logs").addEventListener("click", () => invoke("open_logs_window"));
  $("use-prestep").addEventListener("change", () => {
    $("prestep-box").classList.toggle("hidden", !($input("use-prestep")).checked);
  });
  listen<{ payload: StatusPayload }>("app://status", (e) => updateChip(e.payload.payload));
  invoke<StatusPayload>("get_status").then(updateChip);
}

load().then(wire).catch((e) => console.error(e));
