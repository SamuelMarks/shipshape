import { TestBed } from "@angular/core/testing";

import { CodeMergeComponent } from "./code-merge.component";

describe("CodeMergeComponent", () => {
  it("accepts input updates before the view is initialized", () => {
    TestBed.configureTestingModule({
      imports: [CodeMergeComponent],
    });

    const fixture = TestBed.createComponent(CodeMergeComponent);

    expect(() => {
      fixture.componentRef.setInput("original", "alpha");
      fixture.componentRef.setInput("modified", "beta");
      fixture.detectChanges();
    }).not.toThrow();
  });

  it("renders merge view and emits modified changes when editable", () => {
    TestBed.configureTestingModule({
      imports: [CodeMergeComponent],
    });

    const fixture = TestBed.createComponent(CodeMergeComponent);
    fixture.componentRef.setInput("original", "alpha");
    fixture.componentRef.setInput("modified", "beta");
    fixture.componentRef.setInput("editable", true);
    fixture.detectChanges();

    const component = fixture.componentInstance as unknown as {
      mergeView: {
        b: {
          state: { doc: { length: number } };
          dispatch: (arg: unknown) => void;
        };
      } | null;
      modifiedChange: { subscribe: (fn: (value: string) => void) => void };
    };

    expect(fixture.nativeElement.querySelector(".cm-mergeView")).toBeTruthy();

    const spy = jasmine.createSpy("modifiedChange");
    component.modifiedChange.subscribe(spy);

    const view = component.mergeView?.b;
    view?.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: "gamma" },
    });

    expect(spy).toHaveBeenCalledWith("gamma");
  });

  it("does not emit modified changes when read-only", () => {
    TestBed.configureTestingModule({
      imports: [CodeMergeComponent],
    });

    const fixture = TestBed.createComponent(CodeMergeComponent);
    fixture.componentRef.setInput("original", "alpha");
    fixture.componentRef.setInput("modified", "beta");
    fixture.componentRef.setInput("editable", false);
    fixture.detectChanges();

    const component = fixture.componentInstance as unknown as {
      mergeView: {
        b: {
          state: { doc: { length: number } };
          dispatch: (arg: unknown) => void;
        };
      } | null;
      modifiedChange: { subscribe: (fn: (value: string) => void) => void };
    };

    const spy = jasmine.createSpy("modifiedChange");
    component.modifiedChange.subscribe(spy);

    const view = component.mergeView?.b;
    view?.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: "gamma" },
    });

    expect(spy).not.toHaveBeenCalled();
  });

  it("reconfigures language, editable state, and content on changes", () => {
    TestBed.configureTestingModule({
      imports: [CodeMergeComponent],
    });

    const fixture = TestBed.createComponent(CodeMergeComponent);
    fixture.componentRef.setInput("original", "alpha");
    fixture.componentRef.setInput("modified", "beta");
    fixture.detectChanges();

    const component = fixture.componentInstance as unknown as {
      mergeView: {
        a: { dispatch: jasmine.Spy; state: { doc: { toString(): string } } };
        b: { dispatch: jasmine.Spy; state: { doc: { toString(): string } } };
      } | null;
    };

    const mergeView = component.mergeView as {
      a: { dispatch: jasmine.Spy; state: { doc: { toString(): string } } };
      b: { dispatch: jasmine.Spy; state: { doc: { toString(): string } } };
    };

    spyOn(mergeView.a, "dispatch");
    spyOn(mergeView.b, "dispatch");

    fixture.componentRef.setInput("language", "markdown");
    fixture.componentRef.setInput("editable", true);
    fixture.componentRef.setInput("original", "alpha updated");
    fixture.componentRef.setInput("modified", "beta updated");
    fixture.detectChanges();

    expect(mergeView.a.dispatch).toHaveBeenCalled();
    expect(mergeView.b.dispatch).toHaveBeenCalled();

    mergeView.a.dispatch.calls.reset();
    mergeView.b.dispatch.calls.reset();

    fixture.componentRef.setInput("original", mergeView.a.state.doc.toString());
    fixture.componentRef.setInput("modified", mergeView.b.state.doc.toString());
    fixture.detectChanges();

    expect(mergeView.a.dispatch).not.toHaveBeenCalled();
    expect(mergeView.b.dispatch).not.toHaveBeenCalled();
  });

  it("destroys the merge view on destroy", () => {
    TestBed.configureTestingModule({
      imports: [CodeMergeComponent],
    });

    const fixture = TestBed.createComponent(CodeMergeComponent);
    fixture.componentRef.setInput("original", "alpha");
    fixture.componentRef.setInput("modified", "beta");
    fixture.detectChanges();

    const component = fixture.componentInstance as unknown as {
      mergeView: {
        destroy?: jasmine.Spy;
        a: { destroy: jasmine.Spy };
        b: { destroy: jasmine.Spy };
      } | null;
      ngOnDestroy: () => void;
    };

    const mergeView = component.mergeView as {
      destroy?: jasmine.Spy;
      a: { destroy: jasmine.Spy };
      b: { destroy: jasmine.Spy };
    };

    const mergeViewAny = mergeView as { destroy?: () => void };
    if (mergeViewAny.destroy) {
      spyOn(mergeViewAny as any, "destroy");
    }
    spyOn(mergeView.a, "destroy");
    spyOn(mergeView.b, "destroy");

    component.ngOnDestroy();

    if (mergeViewAny.destroy) {
      expect(mergeViewAny.destroy).toHaveBeenCalled();
    }
    expect(mergeView.a.destroy).toHaveBeenCalled();
    expect(mergeView.b.destroy).toHaveBeenCalled();
  });
});
