/**
 * @file CodeMirror 6 single-document editor component for Angular.
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
import { EditorView } from "@codemirror/view";
import { Compartment, EditorState, Extension } from "@codemirror/state";
import { basicSetup } from "codemirror";

import { resolveLanguageExtension } from "./code-language";

/**
 * Wraps a CodeMirror 6 {@link EditorView} with reactive inputs for content,
 * language syntax highlighting, and read-only mode.
 */
@Component({
  selector: "shipshape-code-editor",
  template: '<div class="editor-host" #host></div>',
  styleUrl: "./code-editor.component.css",
})
export class CodeEditorComponent implements AfterViewInit, OnDestroy {
  /** Two-way bound document content string. */
  readonly content = model.required<string>();
  /** Language identifier used for syntax highlighting. */
  readonly language = input("plaintext");
  /** Whether the editor is read-only. */
  readonly readOnly = input(false);

  /** Host DOM container element reference. */
  readonly host = viewChild.required<ElementRef<HTMLDivElement>>("host");

  /** Underlying CodeMirror editor view instance. */
  private view: EditorView | null = null;
  /** Compartment managing dynamic language extension reconfiguration. */
  private readonly languageCompartment = new Compartment();
  /** Compartment managing dynamic editable state reconfiguration. */
  private readonly editableCompartment = new Compartment();
  /** Signal set to `true` once `ngAfterViewInit` has created the view. */
  private readonly viewReady = signal(false);

  /** Effect synchronizing external `content` signal changes into the editor document. */
  private readonly syncContent = effect(() => {
    if (!this.viewReady()) {
      return;
    }
    const view = this.view as EditorView;
    const content = this.content();
    const current = view.state.doc.toString();
    if (current !== content) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: content },
      });
    }
  });

  /** Effect reconfiguring syntax highlighting when `language` changes. */
  private readonly syncLanguage = effect(() => {
    if (!this.viewReady()) {
      return;
    }
    this.view?.dispatch({
      effects: this.languageCompartment.reconfigure(
        resolveLanguageExtension(this.language()),
      ),
    });
  });

  /** Effect reconfiguring editor read-only state when `readOnly` changes. */
  private readonly syncReadOnly = effect(() => {
    if (!this.viewReady()) {
      return;
    }
    this.view?.dispatch({
      effects: this.editableCompartment.reconfigure(
        EditorView.editable.of(!this.readOnly()),
      ),
    });
  });

  /**
   * Initialize the CodeMirror {@link EditorView} once the host element is mounted.
   */
  ngAfterViewInit(): void {
    this.view = new EditorView({
      state: EditorState.create({
        doc: this.content(),
        extensions: this.buildExtensions(),
      }),
      parent: this.host().nativeElement,
    });
    this.viewReady.set(true);
  }

  /**
   * Destroy the CodeMirror {@link EditorView} when the component is torn down.
   */
  ngOnDestroy(): void {
    this.view?.destroy();
  }

  /**
   * Build the initial CodeMirror extension array.
   *
   * @returns Array of CodeMirror state/view extensions.
   */
  private buildExtensions(): Extension[] {
    return [
      basicSetup,
      this.languageCompartment.of(resolveLanguageExtension(this.language())),
      this.editableCompartment.of(EditorView.editable.of(!this.readOnly())),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          this.content.set(update.state.doc.toString());
        }
      }),
    ];
  }
}
