import { parseXar } from "../format/xar";
import type { ProjectDocument } from "../format/types";
// An original, bundled example; it contains no vendor scripts or artwork.
const xml = `<ChoregrapheProject><Box id="-1" name="Welcome behavior">
<Input id="1" name="onStart" type="1" nature="2"/><Output id="2" name="onStopped" type="1" nature="1"/>
<Timeline enable="0"><BehaviorLayer name="Flow"><BehaviorKeyframe name="Start" index="1"><Diagram scale="100">
<Box id="1" name="Say hello" x="140" y="130" tooltip="A simple script box.&#x0A;Double-click to inspect its Python source.">
<Input id="1" name="onStart" type="1" nature="2"/><Output id="2" name="onStopped" type="1" nature="1"/>
<Parameter id="3" name="Message" content_type="3" value="Hello, world!"/>
<script><content><![CDATA[# Original example — no robot calls are made.\nclass Greeting:\n    def message(self):\n        return "Hello, world!"\n]]></content></script></Box>
<Box id="2" name="Sequence" x="440" y="130" tooltip="A nested behavior. Double-click to explore.">
<Input id="1" name="onStart" type="1" nature="2"/><Output id="2" name="onStopped" type="1" nature="1"/>
<Timeline enable="0"><BehaviorLayer name="Flow"><BehaviorKeyframe name="Inside" index="1"><Diagram>
<Box id="1" name="Nested step" x="180" y="140"><Input id="1" name="onStart" type="1" nature="2"/><Output id="2" name="onStopped" type="1" nature="1"/><script><content><![CDATA[# A box ID is local to its diagram.\nvalue = 42\n]]></content></script></Box>
<Link outputowner="0" indexofoutput="1" inputowner="1" indexofinput="1"/><Link outputowner="1" indexofoutput="2" inputowner="0" indexofinput="2"/>
</Diagram></BehaviorKeyframe></BehaviorLayer></Timeline></Box>
<Box id="3" name="Gesture timeline" x="740" y="130" tooltip="Explore behavior keyframes and motion-key positions.">
<Input id="1" name="onStart" type="1" nature="2"/><Output id="2" name="onStopped" type="1" nature="1"/>
<Timeline enable="1" fps="25" size="120"><BehaviorLayer name="Behavior">
<BehaviorKeyframe name="Prepare" index="1"><Diagram><Box id="1" name="Prepare pose" x="180" y="120"><Input id="1" name="onStart" type="1" nature="2"/></Box></Diagram></BehaviorKeyframe>
<BehaviorKeyframe name="Finish" index="60"><Diagram><Box id="1" name="Finish pose" x="380" y="120"><Input id="1" name="onStart" type="1" nature="2"/></Box></Diagram></BehaviorKeyframe>
</BehaviorLayer><ActuatorList><ActuatorCurve actuator="HeadYaw"><Key frame="1"/><Key frame="30"/><Key frame="60"/><Key frame="120"/></ActuatorCurve></ActuatorList></Timeline></Box>
<Box id="4" name="A long descriptive box title" choren_category="timer" x="440" y="400" tooltip="Category colors and labels stay visible when a long title is shortened."><Input id="1" name="onStart" type="1" nature="2"/><Parameter id="3" name="Delay (seconds)" content_type="2" value="2.5"/><Resource name="Example resource" type="Lock" timeout="1"/></Box>
<Link outputowner="0" indexofoutput="1" inputowner="1" indexofinput="1"/><Link outputowner="1" indexofoutput="2" inputowner="2" indexofinput="1"/>
<Link outputowner="2" indexofoutput="2" inputowner="3" indexofinput="1"/><Link outputowner="3" indexofoutput="2" inputowner="0" indexofinput="2"/>
</Diagram></BehaviorKeyframe></BehaviorLayer></Timeline></Box></ChoregrapheProject>`;
export function exampleProject(): ProjectDocument {
  const root = parseXar(xml);
  return {
    id: "example:welcome",
    source: "Bundled example",
    name: "Welcome to Choren",
    format: "xar",
    behaviors: [
      { id: "welcome", name: "Welcome behavior", source: "welcome.xar", root },
    ],
    files: [{ path: "welcome.xar", kind: "behavior", behaviorId: "welcome" }],
    metadata: {
      Description: "An original example for exploring the read-only workspace.",
    },
    diagnostics: [],
  };
}
