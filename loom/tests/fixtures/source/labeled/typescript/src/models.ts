export interface Entity {
  id: number;
  save(): boolean;
}

export class Task implements Entity {
  constructor(public id: number, public title: string) {}

  save(): boolean {
    return this.validate();
  }

  validate(): boolean {
    return this.title.length > 0;
  }

  get label(): string {
    return `#${this.id}`;
  }

  set label(value: string) {
    this.title = value;
  }
}

export class Note implements Entity {
  constructor(public id: number, public body: string) {}

  save(): boolean {
    return this.body.length > 0;
  }
}
