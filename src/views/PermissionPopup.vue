<script setup lang="ts">
import { ref, computed, reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

const params = new URLSearchParams(window.location.search);
const tool = ref(decodeURIComponent(params.get("tool") || "Unknown"));
const rawInput = ref(decodeURIComponent(params.get("input") || ""));
const sessionId = ref(decodeURIComponent(params.get("session") || ""));
const requestId = ref(decodeURIComponent(params.get("request_id") || ""));
const selectedBehavior = ref("allow");
const submitError = ref("");

// Forward logs to the Rust side so they survive the popup window closing.
function backendLog(level: "info" | "warn" | "error", message: string) {
  invoke("frontend_log", { level, message }).catch(() => {});
}

interface QuestionOption {
  label: string;
  description: string;
}

interface Question {
  header: string;
  question: string;
  multiSelect: boolean;
  options: QuestionOption[];
}

interface ParsedQuestions {
  questions: Question[];
}

const isAskUserQuestion = computed(() => tool.value === "AskUserQuestion");

const parsedQuestions = computed<Question[]>(() => {
  if (!isAskUserQuestion.value) return [];
  try {
    const obj = JSON.parse(rawInput.value) as ParsedQuestions;
    return obj.questions || [];
  } catch {
    return [];
  }
});

// Reactive answers: key = question index, value = selected label(s)
const singleAnswers = reactive<Record<number, string>>({});
const multiAnswers = reactive<Record<number, Set<string>>>({});
const otherTexts = reactive<Record<number, string>>({});

function toggleMulti(qIdx: number, label: string) {
  if (!multiAnswers[qIdx]) multiAnswers[qIdx] = new Set();
  if (multiAnswers[qIdx].has(label)) {
    multiAnswers[qIdx].delete(label);
  } else {
    multiAnswers[qIdx].add(label);
  }
}

function isMultiChecked(qIdx: number, label: string): boolean {
  return multiAnswers[qIdx]?.has(label) || false;
}

// --- Permission tool display fields ---
interface DisplayField {
  label: string;
  value: string;
  mono?: boolean;
}

const displayFields = computed<DisplayField[]>(() => {
  if (isAskUserQuestion.value) return [];
  let obj: Record<string, unknown>;
  try {
    obj = JSON.parse(rawInput.value);
  } catch {
    return rawInput.value ? [{ label: "Input", value: rawInput.value }] : [];
  }

  const fields: DisplayField[] = [];
  const t = tool.value;

  if (t === "Bash" && obj.command) {
    fields.push({ label: "Command", value: String(obj.command), mono: true });
    if (obj.description) fields.push({ label: "Description", value: String(obj.description) });
  } else if ((t === "Write" || t === "Read") && obj.file_path) {
    fields.push({ label: "File", value: String(obj.file_path), mono: true });
    if (obj.content) {
      const content = String(obj.content);
      fields.push({ label: "Content", value: content.length > 200 ? content.slice(0, 200) + "..." : content, mono: true });
    }
  } else if (t === "Edit" && obj.file_path) {
    fields.push({ label: "File", value: String(obj.file_path), mono: true });
    if (obj.old_string) {
      const old = String(obj.old_string);
      fields.push({ label: "Replace", value: old.length > 120 ? old.slice(0, 120) + "..." : old, mono: true });
    }
    if (obj.new_string) {
      const ns = String(obj.new_string);
      fields.push({ label: "With", value: ns.length > 120 ? ns.slice(0, 120) + "..." : ns, mono: true });
    }
  } else if (t === "WebFetch" || t === "WebSearch") {
    if (obj.url) fields.push({ label: "URL", value: String(obj.url) });
    if (obj.query) fields.push({ label: "Query", value: String(obj.query) });
  } else {
    for (const [k, v] of Object.entries(obj)) {
      if (v === null || v === undefined) continue;
      const val = typeof v === "string" ? v : JSON.stringify(v);
      const display = val.length > 150 ? val.slice(0, 150) + "..." : val;
      fields.push({ label: k.replace(/_/g, " "), value: display });
      if (fields.length >= 4) break;
    }
  }

  return fields;
});

const toolColor = computed(() => {
  if (isAskUserQuestion.value) return "#8b5cf6";
  const dangerous = ["Bash", "Write", "Edit", "NotebookEdit"];
  if (dangerous.includes(tool.value)) return "#e85d04";
  return "#2563eb";
});

async function respond(behavior: string, message?: string, updatedInput?: Record<string, unknown>) {
  backendLog("info", `popup respond: behavior=${behavior} requestId=${requestId.value} updatedInput=${JSON.stringify(updatedInput ?? null)}`);
  try {
    await invoke("respond_permission", {
      requestId: requestId.value,
      behavior,
      message: message || null,
      updatedInput: updatedInput || null,
    });
  } catch (e) {
    // Keep the window open and surface the error instead of silently closing.
    submitError.value = String(e);
    backendLog("error", `popup respond_permission failed: ${String(e)}`);
    return;
  }
  const win = getCurrentWebviewWindow();
  await win.close();
}

function submitPermission() {
  respond(selectedBehavior.value);
}

function submitAnswer() {
  try {
    const answers: Record<string, string> = {};
    for (let i = 0; i < parsedQuestions.value.length; i++) {
      const q = parsedQuestions.value[i];
      if (q.multiSelect) {
        answers[q.question] = Array.from(multiAnswers[i] || []).join(", ");
      } else {
        const sel = singleAnswers[i];
        answers[q.question] = sel === "__other__" ? otherTexts[i] || "" : sel || "";
      }
    }
    backendLog("info", `popup submitAnswer built answers: ${JSON.stringify(answers)}`);
    // updatedInput must satisfy AskUserQuestion's input schema, which requires
    // the original `questions` array; CC rejects the override otherwise.
    respond("allow", undefined, { questions: parsedQuestions.value, answers });
  } catch (e) {
    submitError.value = String(e);
    backendLog("error", `popup submitAnswer threw: ${String(e)}`);
  }
}

async function closeWindow() {
  const win = getCurrentWebviewWindow();
  await win.close();
}

async function openSettings() {
  const existing = await WebviewWindow.getByLabel("main");
  if (existing) { await existing.setFocus(); }
  else { new WebviewWindow("main", { url: "/?view=main", title: "CC Remote Seamless", width: 600, height: 400 }); }
}
</script>

<template>
  <div class="permission-popup">
    <div class="titlebar" data-tauri-drag-region>
      <span class="titlebar-text" data-tauri-drag-region>CC Remote Seamless</span>
      <div class="titlebar-actions">
        <button class="titlebar-btn" title="Settings" @click="openSettings">⚙</button>
        <button class="titlebar-btn close" title="Close" @click="closeWindow">✕</button>
      </div>
    </div>

    <div class="body">
      <div class="header">
        <span class="title">{{ isAskUserQuestion ? 'Question' : 'Permission Request' }}</span>
        <span class="session">{{ sessionId.slice(0, 8) }}</span>
      </div>

      <!-- AskUserQuestion mode -->
      <template v-if="isAskUserQuestion && parsedQuestions.length > 0">
        <div v-for="(q, qIdx) in parsedQuestions" :key="qIdx" class="question-block">
          <div class="question-header">{{ q.header }}</div>
          <div class="question-text">{{ q.question }}</div>

          <!-- Single select -->
          <div v-if="!q.multiSelect" class="option-group">
            <label v-for="(opt, oIdx) in q.options" :key="oIdx" class="option-item">
              <input type="radio" :name="'q' + qIdx" :value="opt.label" v-model="singleAnswers[qIdx]" />
              <div class="option-content">
                <span class="option-label">{{ opt.label }}</span>
                <span class="option-desc">{{ opt.description }}</span>
              </div>
            </label>
            <label class="option-item">
              <input type="radio" :name="'q' + qIdx" value="__other__" v-model="singleAnswers[qIdx]" />
              <div class="option-content">
                <span class="option-label">Other</span>
                <input
                  v-if="singleAnswers[qIdx] === '__other__'"
                  type="text"
                  class="other-input"
                  placeholder="Type your answer..."
                  v-model="otherTexts[qIdx]"
                  @click.stop
                />
                <span v-else class="option-desc">Custom text input</span>
              </div>
            </label>
          </div>

          <!-- Multi select -->
          <div v-else class="option-group">
            <label v-for="(opt, oIdx) in q.options" :key="oIdx" class="option-item">
              <input type="checkbox" :checked="isMultiChecked(qIdx, opt.label)" @change="toggleMulti(qIdx, opt.label)" />
              <div class="option-content">
                <span class="option-label">{{ opt.label }}</span>
                <span class="option-desc">{{ opt.description }}</span>
              </div>
            </label>
          </div>
        </div>
        <div v-if="submitError" class="submit-error">提交失败：{{ submitError }}</div>
        <button class="btn submit sticky-submit" @click="submitAnswer">Submit</button>
      </template>

      <!-- Permission mode -->
      <template v-else-if="!isAskUserQuestion">
        <div class="tool-row">
          <span class="tool-dot" :style="{ background: toolColor }"></span>
          <span class="tool-name">{{ tool }}</span>
        </div>

        <div class="fields">
          <div v-for="(f, i) in displayFields" :key="i" class="field">
            <span class="field-label">{{ f.label }}</span>
            <span class="field-value" :class="{ mono: f.mono }">{{ f.value }}</span>
          </div>
        </div>

        <div class="actions">
          <div class="radio-group">
            <label class="radio-option">
              <input type="radio" value="allow" v-model="selectedBehavior" />
              <span class="radio-label allow-label">Allow</span>
            </label>
            <label class="radio-option">
              <input type="radio" value="allowAlways" v-model="selectedBehavior" />
              <span class="radio-label always-label">Always Allow</span>
            </label>
            <label class="radio-option">
              <input type="radio" value="deny" v-model="selectedBehavior" />
              <span class="radio-label deny-label">Deny</span>
            </label>
          </div>
          <button class="btn submit" @click="submitPermission">Confirm</button>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
* { box-sizing: border-box; margin: 0; padding: 0; }

.permission-popup {
  background: #ffffff;
  color: #1f2937;
  height: 100vh;
  display: flex;
  flex-direction: column;
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  overflow: hidden;
}

.titlebar {
  height: 30px;
  display: flex;
  align-items: center;
  padding: 0 12px;
  background: #f8fafc;
  cursor: grab;
  user-select: none;
  -webkit-user-select: none;
  border-bottom: 1px solid #e2e8f0;
  flex-shrink: 0;
}
.titlebar-text { font-size: 11px; font-weight: 500; color: #94a3b8; }
.titlebar-actions { margin-left: auto; display: flex; gap: 4px; }
.titlebar-btn { width: 22px; height: 22px; border: none; border-radius: 4px; background: transparent; color: #64748b; font-size: 12px; cursor: pointer; display: flex; align-items: center; justify-content: center; }
.titlebar-btn:hover { background: #e2e8f0; }
.titlebar-btn.close:hover { background: #fee2e2; color: #ef4444; }

.body {
  flex: 1;
  display: flex;
  flex-direction: column;
  padding: 14px 16px;
  gap: 10px;
  overflow-y: auto;
}

.header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
}
.title { font-weight: 600; font-size: 14px; }
.session { font-size: 10px; color: #94a3b8; font-family: monospace; }

/* Question mode */
.question-block {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.question-header {
  font-size: 10px;
  font-weight: 700;
  color: #8b5cf6;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.question-text {
  font-size: 13px;
  font-weight: 500;
  color: #1f2937;
}
.option-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.option-item {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 8px 10px;
  border: 1px solid #e2e8f0;
  border-radius: 6px;
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s;
}
.option-item:hover { border-color: #a5b4fc; background: #f8fafc; }
.option-item input[type="radio"],
.option-item input[type="checkbox"] {
  width: 14px; height: 14px; margin-top: 2px; accent-color: #3b82f6; flex-shrink: 0;
}
.option-content { display: flex; flex-direction: column; gap: 2px; }
.option-label { font-size: 12px; font-weight: 600; color: #1f2937; }
.option-desc { font-size: 11px; color: #64748b; line-height: 1.3; }
.other-input {
  width: 100%;
  padding: 5px 8px;
  border: 1px solid #cbd5e1;
  border-radius: 4px;
  font-size: 12px;
  margin-top: 4px;
  outline: none;
}
.other-input:focus { border-color: #3b82f6; }

/* Permission mode */
.tool-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.tool-dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
.tool-name { font-weight: 700; font-size: 14px; color: #1f2937; }

.fields {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-height: 0;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.field-label {
  font-size: 10px;
  font-weight: 600;
  color: #64748b;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.field-value {
  font-size: 12px;
  color: #334155;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 4px;
  padding: 5px 8px;
  word-break: break-all;
  white-space: pre-wrap;
  max-height: 80px;
  overflow-y: auto;
  line-height: 1.4;
}
.field-value.mono {
  font-family: 'JetBrains Mono', 'Fira Code', 'SF Mono', monospace;
  font-size: 11px;
}

.actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
  flex-shrink: 0;
  padding-top: 4px;
}
.radio-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.radio-option {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  padding: 6px 8px;
  border-radius: 6px;
  transition: background 0.15s;
}
.radio-option:hover { background: #f1f5f9; }
.radio-option input[type="radio"] { width: 14px; height: 14px; accent-color: #3b82f6; margin: 0; }
.radio-label { font-size: 12px; font-weight: 600; }
.allow-label { color: #16a34a; }
.always-label { color: #2563eb; }
.deny-label { color: #dc2626; }

.btn {
  padding: 9px 12px;
  border: none;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  transition: opacity 0.15s;
}
.btn:hover { opacity: 0.85; }
.btn:active { transform: scale(0.97); }
.btn.submit { background: #3b82f6; color: #fff; }
.sticky-submit {
  position: sticky;
  bottom: 0;
  margin-top: 4px;
  box-shadow: 0 -6px 12px -8px rgba(0, 0, 0, 0.25);
}
.submit-error {
  font-size: 11px;
  color: #ef4444;
  background: #fef2f2;
  border: 1px solid #fecaca;
  border-radius: 6px;
  padding: 6px 8px;
  word-break: break-all;
}
</style>
