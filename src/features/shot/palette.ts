import type { Box, Handle, Point, Shape } from "./annotate";

export const shotColors = [
	"#E23B3B",
	"#E6A317",
	"#2F9E6B",
	"#2F6FED",
	"#111111",
	"#FFFFFF",
];

export const shotTools = [
	{ id: "move", label: "移动", key: "V" },
	{ id: "rect", label: "矩形", key: "R" },
	{ id: "arrow", label: "箭头", key: "A" },
	{ id: "pen", label: "画笔", key: "P" },
	{ id: "text", label: "文字", key: "T" },
] as const;

export type ShotTool = (typeof shotTools)[number]["id"];

export type ShotDrag =
	| {
			kind: "select";
			origin: Point;
			point: Point;
			square: boolean;
			previousSelection: Box | null;
			previousShapes: Shape[];
			cleared: boolean;
	  }
	| { kind: "move"; origin: Point; point: Point; box: Box; shapes: Shape[] }
	| { kind: "resize"; handle: Handle; point: Point; box: Box; square: boolean }
	| {
			kind: "rect" | "arrow" | "pen";
			origin: Point;
			point: Point;
			points: Point[];
			square: boolean;
	  };
