import { Component, type ReactNode } from "react";

import { i18n } from "../i18n";
import { errorMessage } from "../shared/format";

export class ErrorBoundary extends Component<{ children: ReactNode }, { error: string }> {
  state = { error: "" };

  static getDerivedStateFromError(error: unknown) {
    return { error: errorMessage(error) };
  }

  componentDidCatch(error: unknown) {
    console.error(error);
  }

  render() {
    if (this.state.error) {
      return (
        <main className="shellFallback">
          <section className="bootError">
            <h1>{i18n.t("app.renderFailed")}</h1>
            <p>{i18n.t("app.renderFailedDescription")}</p>
            <pre>{this.state.error}</pre>
          </section>
        </main>
      );
    }
    return this.props.children;
  }
}
