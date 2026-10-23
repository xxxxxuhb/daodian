export type ThemeName = "celadon" | "night";
export type WindowTier = "mini" | "standard" | "wide";

export interface WindowFrame {
  w: number;
  h: number;
  x?: number;
  y?: number;
}

export interface Settings {
  theme: ThemeName;
  window: {
    frames: Partial<Record<WindowTier, WindowFrame>>;
    lastTier: WindowTier | "";
  };
  repeat: {
    defaultSkipWeekends: boolean;
    holidays: string[];
    workdays: string[];
  };
  weekReport: {
    template: string;
    rangeOffset: number;
  };
  calendar: {
    enabled: boolean;
    path: string;
    lastStatus: string;
  };
  feishu: WebhookSettings;
  wecom: WebhookSettings;
  quickCapture: {
    enabled: boolean;
    shortcut: string;
    lastStatus: string;
  };
  deepSeek: {
    enabled: boolean;
    baseUrl: string;
    model: string;
    hasApiKey: boolean;
    lastStatus: string;
  };
}

export interface WebhookSettings {
  enabled: boolean;
  webhook: string;
  lastStatus: string;
}
