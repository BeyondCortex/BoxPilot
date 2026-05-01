import { ref } from "vue";

export interface ToastEntry {
  id: number;
  kind: "info" | "success" | "error";
  message: string;
}

const entries = ref<ToastEntry[]>([]);
let nextId = 1;

function push(kind: ToastEntry["kind"], message: string) {
  const id = nextId++;
  entries.value.push({ id, kind, message });
  setTimeout(
    () => {
      entries.value = entries.value.filter((e) => e.id !== id);
    },
    kind === "error" ? 8000 : 4000,
  );
}

export function useToast() {
  return {
    entries,
    info: (m: string) => push("info", m),
    success: (m: string) => push("success", m),
    error: (m: string) => push("error", m),
    dismiss: (id: number) => {
      entries.value = entries.value.filter((e) => e.id !== id);
    },
  };
}
