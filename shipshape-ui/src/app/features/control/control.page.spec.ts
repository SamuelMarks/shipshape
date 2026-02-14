import { TestBed } from "@angular/core/testing";
import { of } from "rxjs";

import { ControlRoomPage } from "./control.page";
import { ApiService } from "../../services/api.service";
import {
  ControlOptionsResponse,
  ControlQueueResponse,
} from "../../services/api.models";

const optionsResponse: ControlOptionsResponse = {
  mechanics: [
    {
      id: "cpp-types",
      label: "C++ type safety",
      description: "Enforce strict typing.",
    },
  ],
  activity: [
    {
      id: "LG-1",
      title: "Audit queued",
      detail: "Queued 1 repo.",
      time: "Now",
      tone: "info",
    },
  ],
};

const queueResponse: ControlQueueResponse = {
  runId: "run-1",
  log: {
    id: "LG-2",
    title: "Refit queued",
    detail: "Queued 2 repos.",
    time: "Just now",
    tone: "info",
  },
};

describe("ControlRoomPage", () => {
  it("toggles mechanics and queues runs", () => {
    const queueSpy = jasmine
      .createSpy("queueControlRun")
      .and.returnValue(of(queueResponse));

    TestBed.configureTestingModule({
      imports: [ControlRoomPage],
      providers: [
        {
          provide: ApiService,
          useValue: {
            getControlOptions: () => of(optionsResponse),
            queueControlRun: queueSpy,
          },
        },
      ],
    });

    const fixture = TestBed.createComponent(ControlRoomPage);
    fixture.detectChanges();

    const page = fixture.componentInstance;
    page.toggleMechanic(optionsResponse.mechanics[0]);
    expect(page.selectedMechanics()).not.toContain("cpp-types");

    page.toggleMechanic(optionsResponse.mechanics[0]);
    expect(page.selectedMechanics()).toContain("cpp-types");

    page.queueRun();
    expect(queueSpy).toHaveBeenCalled();
    expect(page.activityLog()[0].id).toBe("LG-2");
  });
});
