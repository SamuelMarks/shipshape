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
import { EditorView } from "@codemirror/view";
import { Compartment, EditorState, Extension } from "@codemirror/state";
import { basicSetup } from "codemirror";

import { resolveLanguageExtension } from "./code-language";

@Component({
  selector: "shipshape-code-editor",
  template: '<div class="editor-host" #host></div>',
  styleUrl: "./code-editor.component.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CodeEditorComponent implements AfterViewInit, OnDestroy {
  readonly content = input.required<string>();
  readonly language = input("plaintext");
  readonly readOnly = input(false);
  readonly contentChange = output<string>();

  @ViewChild("host", { static: true }) host!: ElementRef<HTMLDivElement>;

  private view: EditorView | null = null;
  private readonly languageCompartment = new Compartment();
  private readonly editableCompartment = new Compartment();
  private readonly viewReady = signal(false);
  private readonly syncContent = effect(() => {
    if (!this.viewReady()) {
      return;
    }
    const view = this.view!;
    const content = this.content();
    const current = view.state.doc.toString();
    if (current !== content) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: content },
      });
    }
  });
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

  ngAfterViewInit(): void {
    this.view = new EditorView({
      state: EditorState.create({
        doc: this.content(),
        extensions: this.buildExtensions(),
      }),
      parent: this.host.nativeElement,
    });
    this.viewReady.set(true);
  }

  ngOnDestroy(): void {
    this.view?.destroy();
  }

  private buildExtensions(): Extension[] {
    return [
      basicSetup,
      this.languageCompartment.of(resolveLanguageExtension(this.language())),
      this.editableCompartment.of(EditorView.editable.of(!this.readOnly())),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          this.contentChange.emit(update.state.doc.toString());
        }
      }),
    ];
  }
}
