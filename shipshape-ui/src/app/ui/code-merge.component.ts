/**
 * @file CodeMirror 6 side-by-side merge/diff editor component for Angular.
 */

import {
  AfterViewInit,
  Component,
  ElementRef,
  effect,
  input,
  model,
  OnDestroy,
  signal,
  viewChild,
} from "@angular/core";
import { basicSetup } from "codemirror";
import { Compartment } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { MergeView } from "@codemirror/merge";

import { resolveLanguageExtension } from "./code-language";

/**
 * Wraps a CodeMirror 6 {@link MergeView} comparing `original` (pane A) and
 * `modified` (pane B) documents with synchronized syntax highlighting.
 */
@Component({
  selector: "shipshape-code-merge",
  template: '<div class="merge-host" #host></div>',
  styleUrl: "./code-merge.component.css",
})
export class CodeMergeComponent implements AfterViewInit, OnDestroy {
  /** Read-only baseline document content displayed in pane A. */
  readonly original = input.required<string>();
  /** Two-way bound modified document content displayed in pane B. */
  readonly modified = model.required<string>();
  /** Language identifier used for syntax highlighting in both panes. */
  readonly language = input("plaintext");
  /** Whether pane B (`modified`) allows user editing. */
  readonly editable = input(false);

  /** Host DOM container element reference. */
  readonly host = viewChild.required<ElementRef<HTMLDivElement>>("host");

  /** Underlying CodeMirror MergeView instance. */
  private mergeView: {
    a: EditorView;
    b: EditorView;
    destroy?: () => void;
  } | null = null;
  /** Compartment managing language highlighting in pane A. */
  private readonly languageCompartmentA = new Compartment();
  /** Compartment managing language highlighting in pane B. */
  private readonly languageCompartmentB = new Compartment();
  /** Compartment managing editable state in pane B. */
  private readonly editableCompartment = new Compartment();
  /** Signal set to `true` once `ngAfterViewInit` has created the merge view. */
  private readonly viewReady = signal(false);

  /** Effect reconfiguring syntax highlighting in both panes when `language` changes. */
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

  /** Effect reconfiguring editable state in pane B when `editable` changes. */
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

  /** Effect synchronizing external `original` changes into pane A. */
  private readonly syncOriginal = effect(() => {
    if (!this.viewReady()) {
      return;
    }
    const view = (this.mergeView as { a: EditorView }).a;
    const original = this.original();
    const current = view.state.doc.toString();
    if (current !== original) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: original },
      });
    }
  });

  /** Effect synchronizing external `modified` changes into pane B. */
  private readonly syncModified = effect(() => {
    if (!this.viewReady()) {
      return;
    }
    const view = (this.mergeView as { b: EditorView }).b;
    const modified = this.modified();
    const current = view.state.doc.toString();
    if (current !== modified) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: modified },
      });
    }
  });

  /**
   * Initialize the CodeMirror {@link MergeView} once the host element is mounted.
   */
  ngAfterViewInit(): void {
    this.mergeView = new MergeView({
      parent: this.host().nativeElement,
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
              this.modified.set(update.state.doc.toString());
            }
          }),
        ],
      },
      highlightChanges: true,
      gutter: true,
    }) as { a: EditorView; b: EditorView; destroy?: () => void };
    this.viewReady.set(true);
  }

  /**
   * Destroy the merge view and both child editor views when torn down.
   */
  ngOnDestroy(): void {
    this.mergeView?.destroy?.();
    this.mergeView?.a.destroy();
    this.mergeView?.b.destroy();
  }
}
