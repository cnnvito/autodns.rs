import { Component, type ReactNode } from "react";

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
            <h1>autodns 渲染失败</h1>
            <p>桌面外壳已经加载，但 React 页面发生了错误。</p>
            <pre>{this.state.error}</pre>
          </section>
        </main>
      );
    }
    return this.props.children;
  }
}
