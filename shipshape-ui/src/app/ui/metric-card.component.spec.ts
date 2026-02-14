import { TestBed } from "@angular/core/testing";

import { MetricCardComponent } from "./metric-card.component";

describe("MetricCardComponent", () => {
  it("renders value, detail, and trend", () => {
    TestBed.configureTestingModule({
      imports: [MetricCardComponent],
    });
    const fixture = TestBed.createComponent(MetricCardComponent);
    fixture.componentRef.setInput("label", "Coverage");
    fixture.componentRef.setInput("value", "92%");
    fixture.componentRef.setInput("trend", "+2%");
    fixture.componentRef.setInput("detail", "Last 24h");
    fixture.componentRef.setInput("tone", "good");
    fixture.detectChanges();

    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain("Coverage");
    expect(text).toContain("92%");
    expect(text).toContain("+2%");
    expect(text).toContain("Last 24h");
  });
});
