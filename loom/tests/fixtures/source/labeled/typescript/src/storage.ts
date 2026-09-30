import axios from "axios";
import type { Entity } from "./models";
import { slugify as toKey, MAX_ITEMS } from "./util";

export class Storage {
  private keys: string[] = [];

  add(entity: Entity, name: string): boolean {
    if (this.keys.length >= MAX_ITEMS) {
      return false;
    }
    this.keys.push(toKey(name));
    return entity.save();
  }

  async sync(url: string): Promise<void> {
    await axios.post(url, this.keys);
  }
}
