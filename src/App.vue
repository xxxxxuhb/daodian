<script setup lang="ts">
import AppShell from "./features/shell/AppShell.vue";
import PetApp from "./features/pet/PetApp.vue";

const isPetWindow = new URLSearchParams(location.search).get("window") === "pet";
</script>

<template>
  <PetApp v-if="isPetWindow" />
  <template v-else>
  <svg class="sprite" aria-hidden="true">
    <symbol id="i-clock" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.4" /><path d="M12 7.6v4.9l3.3 1.9" /></symbol>
    <symbol id="i-plus" viewBox="0 0 24 24"><path d="M12 5.5v13M5.5 12h13" /></symbol>
    <symbol id="i-search" viewBox="0 0 24 24"><circle cx="10.8" cy="10.8" r="6.2" /><path d="M15.4 15.4 20 20" /></symbol>
    <symbol id="i-bell" viewBox="0 0 24 24"><path d="M6.6 9.6a5.4 5.4 0 0 1 10.8 0c0 3.6 1.3 4.9 1.3 4.9H5.3s1.3-1.3 1.3-4.9Z" /><path d="M10 18.3a2.1 2.1 0 0 0 4 0" /></symbol>
    <symbol id="i-check" viewBox="0 0 24 24"><path d="m5.5 12.6 4.2 4.2L18.8 7.7" /></symbol>
    <symbol id="i-x" viewBox="0 0 24 24"><path d="M6.5 6.5l11 11M17.5 6.5l-11 11" /></symbol>
    <symbol id="i-cal" viewBox="0 0 24 24"><rect x="4" y="5.6" width="16" height="13.8" rx="2.4" /><path d="M4 10.2h16M8.6 3.8v3.6M15.4 3.8v3.6" /></symbol>
    <symbol id="i-list" viewBox="0 0 24 24"><path d="M8.5 7h10.5M8.5 12h10.5M8.5 17h10.5M5 7h.01M5 12h.01M5 17h.01" /></symbol>
    <symbol id="i-layers" viewBox="0 0 24 24"><path d="M12 4.5 20 9l-8 4.5L4 9l8-4.5Z" /><path d="M4 14l8 4.5 8-4.5" /></symbol>
    <symbol id="i-moon" viewBox="0 0 24 24"><path d="M20 14.5A8.5 8.5 0 0 1 9.5 4 8.5 8.5 0 1 0 20 14.5Z" /></symbol>
    <symbol id="i-timeline" viewBox="0 0 24 24"><path d="M3.5 18.6h17" /><circle cx="8.4" cy="10.4" r="2.1" /><circle cx="16" cy="10.4" r="2.1" /><path d="M10.5 10.4h3.4" /></symbol>
    <symbol id="i-grip" viewBox="0 0 24 24"><path d="M9.5 7h.01M9.5 12h.01M9.5 17h.01M14.5 7h.01M14.5 12h.01M14.5 17h.01" /></symbol>
    <symbol id="i-trash" viewBox="0 0 24 24"><path d="M4.8 7h14.4M9.4 7V4.8h5.2V7M6.8 7l.9 12.2h8.6L17.2 7" /></symbol>
    <symbol id="i-repeat" viewBox="0 0 24 24"><path d="M7 8h9a3 3 0 0 1 0 6h-1" /><path d="m9.5 5.5L7 8l2.5 2.5" /><path d="M17 16H8a3 3 0 0 1 0-6h1" /><path d="m14.5 18.5L17 16l-2.5-2.5" /></symbol>
    <symbol id="i-note" viewBox="0 0 24 24"><path d="M7 4.8h7.2L19 9.4V19a1.6 1.6 0 0 1-1.6 1.6H7.6A1.6 1.6 0 0 1 6 19V6.4A1.6 1.6 0 0 1 7.6 4.8H7Z" /><path d="M14 4.8V9.2h4.6M8.5 13h7M8.5 16.2h4.8" /></symbol>
    <symbol id="i-quote" viewBox="0 0 24 24"><path d="M8 16.6c.5-2.6 2-4.4 4.2-5.2" /><path d="M7.2 11.6h2.8V14.4" /><path d="M14.2 16.6c.5-2.6 2-4.4 4.2-5.2" /><path d="M13.4 11.6H16.2V14.4" /></symbol>
    <symbol id="i-task" viewBox="0 0 24 24"><rect x="4.6" y="5" width="5" height="5" rx="1.1" /><path d="m5.8 7.5 1.1 1.1 2-2.1" /><path d="M12.2 7.5h7" /><rect x="4.6" y="14" width="5" height="5" rx="1.1" /><path d="M12.2 16.5h7" /></symbol>
    <symbol id="i-table" viewBox="0 0 24 24"><rect x="4.4" y="5" width="15.2" height="14" rx="1.6" /><path d="M4.4 9.6h15.2M4.4 14.2h15.2M9.5 5v14M14.6 5v14" /></symbol>
    <symbol id="i-image" viewBox="0 0 24 24"><rect x="4" y="5" width="16" height="14" rx="2" /><circle cx="9" cy="10" r="1.3" /><path d="m4.6 15.6 3.8-3.2 2.8 2.2 2.3-1.8 5.4 4" /></symbol>
    <symbol id="i-link" viewBox="0 0 24 24"><path d="M10 14.2 8.2 16a2.7 2.7 0 0 1-3.8-3.8L6.6 10a2.7 2.7 0 0 1 3.8 0" /><path d="M14 9.8 15.8 8a2.7 2.7 0 0 1 3.8 3.8L17.4 14a2.7 2.7 0 0 1-3.8 0" /></symbol>
    <symbol id="i-mark" viewBox="0 0 24 24"><path d="m14.4 5.2 4.4 4.4-8.2 8.2H6.2v-4.4l8.2-8.2Z" /><path d="m12.6 7 4.4 4.4M6.2 18.2h12" /></symbol>
  </svg>
  <app-shell />
  </template>
</template>
