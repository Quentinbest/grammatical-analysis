import type { VendorId } from "../../domain/modelCatalog.ts";

export interface VendorIconProps {
  readonly vendor: VendorId;
  readonly size?: number;
}

export function VendorIcon({ vendor, size = 16 }: VendorIconProps) {
  switch (vendor) {
    case "openai":
      return (
        <svg
          className="vendor-icon"
          data-vendor="openai"
          width={size}
          height={size}
          viewBox="0 0 16 16"
          fill="none"
          aria-hidden="true"
        >
          <path
            d="M8 1.6a2.9 2.9 0 0 1 2.52 1.46l.18.32a2.9 2.9 0 0 1 2.75 2.02 2.9 2.9 0 0 1-.42 2.6l.18.32a2.9 2.9 0 0 1-2.51 4.35l-.36-.01A2.9 2.9 0 0 1 8 14.4a2.9 2.9 0 0 1-2.52-1.46l-.18-.32a2.9 2.9 0 0 1-2.75-2.02 2.9 2.9 0 0 1 .42-2.6l-.18-.32a2.9 2.9 0 0 1 2.51-4.35l.36.01A2.9 2.9 0 0 1 8 1.6Z"
            stroke="#10a37f"
            strokeWidth="1.35"
            strokeLinejoin="round"
          />
          <polygon
            points="8,5.1 10.5,6.55 10.5,9.45 8,10.9 5.5,9.45 5.5,6.55"
            stroke="#10a37f"
            strokeWidth="1.2"
          />
        </svg>
      );

    case "anthropic":
      return (
        <svg
          className="vendor-icon"
          data-vendor="anthropic"
          width={size}
          height={size}
          viewBox="0 0 16 16"
          fill="none"
          aria-hidden="true"
        >
          <path
            d="M8 1.5v13M2.37 4.75l11.26 6.5M2.37 11.25l11.26-6.5M5.18 2.1l5.64 11.8M10.82 2.1 5.18 13.9"
            stroke="#d97757"
            strokeWidth="1.5"
            strokeLinecap="round"
          />
          <circle cx="8" cy="8" r="1.7" fill="#d97757" />
        </svg>
      );

    case "deepseek":
      return (
        <svg
          className="vendor-icon"
          data-vendor="deepseek"
          width={size}
          height={size}
          viewBox="0 0 16 16"
          fill="none"
          aria-hidden="true"
        >
          <path
            d="M2.2 9.1c0-3.1 2.6-5.3 6.1-5.3 2.5 0 4.4 1.3 5.1 3.1.2.5-.2.9-.7.8-.9-.2-1.8.1-2.3.8-.7.9-.5 2.1.4 2.8-1 .8-2.4 1.2-4 1.2-1.9 0-3.5-.7-4.3-1.9l-1.3.8V8.8l1 .3Z"
            fill="#4d6bfe"
          />
          <circle cx="9.8" cy="6.6" r="0.85" fill="#ffffff" />
        </svg>
      );

    case "gemini":
      return (
        <svg
          className="vendor-icon"
          data-vendor="gemini"
          width={size}
          height={size}
          viewBox="0 0 16 16"
          fill="none"
          aria-hidden="true"
        >
          <path
            d="M8 1.3C8.55 5.1 10.9 7.45 14.7 8 10.9 8.55 8.55 10.9 8 14.7 7.45 10.9 5.1 8.55 1.3 8 5.1 7.45 7.45 5.1 8 1.3Z"
            fill="#5e97f6"
            stroke="#8ab4f8"
            strokeWidth="0.6"
            strokeLinejoin="round"
          />
        </svg>
      );

    case "openrouter":
      return (
        <svg
          className="vendor-icon"
          data-vendor="openrouter"
          width={size}
          height={size}
          viewBox="0 0 16 16"
          fill="none"
          aria-hidden="true"
        >
          <path
            d="M2 8h4.2l2.8-3.8h4.5M6.2 8 9 11.8h4.5"
            stroke="#6366f1"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <path
            d="m11.8 2.6 1.9 1.6-1.9 1.6M11.8 10.2l1.9 1.6-1.9 1.6"
            stroke="#6366f1"
            strokeWidth="1.4"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      );

    case "ollama":
      return (
        <svg
          className="vendor-icon"
          data-vendor="ollama"
          width={size}
          height={size}
          viewBox="0 0 16 16"
          fill="none"
          aria-hidden="true"
        >
          <path
            d="M5.2 5.4V2.8c0-.6.7-.9 1.1-.4l1.1 1.4h1.2l1.1-1.4c.4-.5 1.1-.2 1.1.4v2.6c1 .7 1.6 1.9 1.6 3.3 0 2.5-1.9 4.5-4.4 4.5s-4.4-2-4.4-4.5c0-1.4.6-2.6 1.6-3.3Z"
            stroke="currentColor"
            strokeWidth="1.35"
            strokeLinejoin="round"
          />
          <circle cx="6.6" cy="8.3" r="0.75" fill="currentColor" />
          <circle cx="9.4" cy="8.3" r="0.75" fill="currentColor" />
        </svg>
      );

    case "lmstudio":
      return (
        <svg
          className="vendor-icon"
          data-vendor="lmstudio"
          width={size}
          height={size}
          viewBox="0 0 16 16"
          fill="none"
          aria-hidden="true"
        >
          <polygon
            points="8,1.8 13.5,5 13.5,11 8,14.2 2.5,11 2.5,5"
            stroke="#8b5cf6"
            strokeWidth="1.35"
            strokeLinejoin="round"
          />
          <path
            d="M2.5 5 8 8.2 13.5 5M8 8.2v6"
            stroke="#8b5cf6"
            strokeWidth="1.25"
            strokeLinecap="round"
          />
        </svg>
      );

    case "custom":
      return (
        <svg
          className="vendor-icon"
          data-vendor="custom"
          width={size}
          height={size}
          viewBox="0 0 16 16"
          fill="none"
          aria-hidden="true"
        >
          <rect
            x="3.5"
            y="3.5"
            width="9"
            height="9"
            rx="2"
            stroke="currentColor"
            strokeWidth="1.35"
          />
          <circle cx="8" cy="8" r="1.6" fill="currentColor" />
        </svg>
      );

    default: {
      const _exhaustive: never = vendor;
      return _exhaustive;
    }
  }
}
