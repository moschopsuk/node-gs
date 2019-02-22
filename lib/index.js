const PipeLineEmitter = require('./emitter');

const host = "localhost";
const port = 8088;

const emitter = new PipeLineEmitter(`videotestsrc is-live=true ! x264enc ! mpegtsmux ! tcpserversink recover-policy=keyframe host=${host} port=${port}`);
emitter.on('error', ({ error }) => console.error(error));

console.info(`TCP output created at tcp://${host}:${port}`);
