import * as React from "react";
import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import { motion } from "framer-motion";
window.MotionReact = { React, createRoot, flushSync, motion };
