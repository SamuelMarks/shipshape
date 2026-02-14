import {
  AfterViewInit,
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  effect,
  input,
  OnDestroy,
  output,
  signal,
  ViewChild,
} from "@angular/core";
import { basicSetup } from "codemirror";
import { Compartment } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { MergeView } from "@codemirror/merge";

import { resolveLanguageExtension } from "./code-language";

@Component({
  selector: "shipshape-code-merge",
  template: '<div class="merge-host" #host></div>',
  styleUrl: "./code-merge.component.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CodeMergeComponent implements AfterViewInit, OnDestroy {
  readonly original = input.required<string>();
  readonly modified = input.required<string>();
  readonly language = input("plaintext");
  readonly editable = input(false);
  readonly modifiedChange = output<string>();

  @ViewChild("host", { static: true }) host!: ElementRef<HTMLDivElement>;

  private mergeView: {
    a: EditorView;
    b: EditorView;
    destroy?: () => void;
  } | null = null;
  private readonly languageCompartmentA = new Compartment();
  private readonly languageCompartmentB = new Compartment();
  private readonly editableCompartment = new Compartment();
  private readonly viewReady = signal(false);
  private readonly syncLanguage = effect(() => {
    if (!this.viewReady()) {
      return;
    }
    const extension = resolveLanguageExtension(this.language());
    this.mergeView?.a.dispatch({
      effects: this.languageCompartmentA.reconfigure(extension),
    });
    this.mergeView?.b.dispatch({
      effects: this.languageCompartmentB.reconfigure(extension),
    });
  });
  private readonly syncEditable = effect(() => {
    if (!this.viewReady()) {
      return;
    }
    this.mergeView?.b.dispatch({
      effects: this.editableCompartment.reconfigure(
        EditorView.editable.of(this.editable()),
      ),
    });
  });
  private readonly syncOriginal = effect(() => {
    if (!this.viewReady()) {
      return;
    }
    const view = this.mergeView?.a;
    if (!view) {
      return;
    }
    const original = this.original();
    const current = view.state.doc.toString();
    if (current !== original) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: original },
      });
    }
  });
  private readonly syncModified = effect(() => {
    if (!this.viewReady()) {
      return;
    }
    const view = this.mergeView?.b;
    if (!view) {
      return;
    }
    const modified = this.modified();
    const current = view.state.doc.toString();
    if (current !== modified) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: modified },
      });
    }
  });

  ngAfterViewInit(): void {
    this.mergeView = new MergeView({
      parent: this.host.nativeElement,
      a: {
        doc: this.original(),
        extensions: [
          basicSetup,
          this.languageCompartmentA.of(
            resolveLanguageExtension(this.language()),
          ),
          EditorView.editable.of(false),
        ],
      },
      b: {
        doc: this.modified(),
        extensions: [
          basicSetup,
          this.languageCompartmentB.of(
            resolveLanguageExtension(this.language()),
          ),
          this.editableCompartment.of(EditorView.editable.of(this.editable())),
          EditorView.updateListener.of((update) => {
            if (!this.editable()) {
              return;
            }
            if (update.docChanged) {
              this.modifiedChange.emit(update.state.doc.toString());
            }
          }),
        ],
      },
      highlightChanges: true,
      gutter: true,
    }) as { a: EditorView; b: EditorView; destroy?: () => void };
    this.viewReady.set(true);
  }

  ngOnDestroy(): void {
    this.mergeView?.destroy?.();
    this.mergeView?.a.destroy();
    this.mergeView?.b.destroy();
  }
}
