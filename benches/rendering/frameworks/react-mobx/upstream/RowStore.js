// Vendored from krausest/js-framework-benchmark@f2df01a (frameworks/keyed/react-mobX/src/RowStore.js), Apache-2.0.
import { makeObservable, observable } from 'mobx';

export class RowStore {
  id;

  label;

  constructor(id, label) {
    makeObservable(this, {
      label: observable,
    });

    this.id = id;
    this.label = label;
  }
}
