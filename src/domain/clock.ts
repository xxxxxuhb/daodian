import { onScopeDispose, ref } from "vue";

const now = ref(Date.now());
let users = 0;
let timer = 0;

export function useNow() {
  users += 1;
  if (users === 1) {
    timer = window.setInterval(() => {
      now.value = Date.now();
    }, 1_000);
  }
  onScopeDispose(() => {
    users -= 1;
    if (users <= 0) {
      users = 0;
      window.clearInterval(timer);
    }
  });
  return now;
}
