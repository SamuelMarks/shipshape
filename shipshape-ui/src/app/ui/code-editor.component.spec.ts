import { TestBed } from "@angular/core/testing";

import { CodeEditorComponent } from "./code-editor.component";

describe("CodeEditorComponent", () => {
  it("accepts input updates before the view is initialized", () => {
    TestBed.configureTestingModule({
      imports: [CodeEditorComponent],
    });

    const fixture = TestBed.createComponent(CodeEditorComponent);

    expect(() => {
      fixture.componentRef.setInput("content", "alpha");
      fixture.detectChanges();
    }).not.toThrow();
  });

  it("renders and syncs content", () => {
    TestBed.configureTestingModule({
      imports: [CodeEditorComponent],
    });

    const fixture = TestBed.createComponent(CodeEditorComponent);
    fixture.componentRef.setInput("content", "alpha");
    fixture.componentRef.setInput("language", "markdown");
    fixture.detectChanges();

    const component = fixture.componentInstance as unknown as {
      view: { state: { doc: { toString(): string } } } | null;
    };

    expect(fixture.nativeElement.querySelector(".cm-editor")).toBeTruthy();
    expect(component.view?.state.doc.toString()).toBe("alpha");

    fixture.componentRef.setInput("content", "beta");
    fixture.detectChanges();

    expect(component.view?.state.doc.toString()).toBe("beta");
  });

  it("reconfigures language and readOnly settings", () => {
    TestBed.configureTestingModule({
      imports: [CodeEditorComponent],
    });

    const fixture = TestBed.createComponent(CodeEditorComponent);
    fixture.componentRef.setInput("content", "alpha");
    fixture.detectChanges();

    const component = fixture.componentInstance as unknown as {
      view: { dispatch: jasmine.Spy } | null;
    };

    const view = component.view as { dispatch: jasmine.Spy };
    spyOn(view, "dispatch");

    fixture.componentRef.setInput("language", "markdown");
    fixture.componentRef.setInput("readOnly", true);
    fixture.detectChanges();

    expect(view.dispatch).toHaveBeenCalled();
  });

  it("skips content updates when the value is unchanged", () => {
    TestBed.configureTestingModule({
      imports: [CodeEditorComponent],
    });

    const fixture = TestBed.createComponent(CodeEditorComponent);
    fixture.componentRef.setInput("content", "same");
    fixture.detectChanges();

    const component = fixture.componentInstance as unknown as {
      view: {
        dispatch: jasmine.Spy;
        state: { doc: { toString(): string } };
      } | null;
    };

    const view = component.view as {
      dispatch: jasmine.Spy;
      state: { doc: { toString(): string } };
    };
    spyOn(view, "dispatch");

    fixture.componentRef.setInput("content", view.state.doc.toString());
    fixture.detectChanges();

    expect(view.dispatch).not.toHaveBeenCalled();
  });

  it("emits changes on edits", () => {
    TestBed.configureTestingModule({
      imports: [CodeEditorComponent],
    });

    const fixture = TestBed.createComponent(CodeEditorComponent);
    fixture.componentRef.setInput("content", "alpha");
    fixture.detectChanges();

    const component = fixture.componentInstance as unknown as {
      view: {
        state: { doc: { length: number } };
        dispatch: (arg: unknown) => void;
      } | null;
      contentChange: { subscribe: (fn: (value: string) => void) => void };
    };

    const spy = jasmine.createSpy("contentChange");
    component.contentChange.subscribe(spy);

    const view = component.view;
    view?.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: "delta" },
    });

    expect(spy).toHaveBeenCalledWith("delta");
  });

  it("destroys the editor view on destroy", () => {
    TestBed.configureTestingModule({
      imports: [CodeEditorComponent],
    });

    const fixture = TestBed.createComponent(CodeEditorComponent);
    fixture.componentRef.setInput("content", "alpha");
    fixture.detectChanges();

    const component = fixture.componentInstance as unknown as {
      view: { destroy: jasmine.Spy } | null;
      ngOnDestroy: () => void;
    };

    const view = component.view as { destroy: jasmine.Spy };
    spyOn(view, "destroy");

    component.ngOnDestroy();

    expect(view.destroy).toHaveBeenCalled();
  });
});
