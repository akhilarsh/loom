import React from "react";

type PanelProps = { title: string; children?: React.ReactNode };

export class Panel extends React.Component<PanelProps> {
  header() {
    return <h2>{this.props.title}</h2>;
  }

  render() {
    return (
      <section>
        {this.header()}
        {this.props.children}
      </section>
    );
  }
}

export class Sidebar extends Panel {
  header() {
    return <h3>{this.props.title}</h3>;
  }
}

export function heading(panel: Panel) {
  return panel.header();
}
