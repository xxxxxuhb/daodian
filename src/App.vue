<script setup lang="ts">
import AppShell from "./features/shell/AppShell.vue";
</script>

<template>
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
  </svg>
  <app-shell />
</template>
