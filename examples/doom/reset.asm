; SPDX-License-Identifier: GPL-2.0-or-later
; Doom port: reset entry (algebraic syntax, assembled with asm55). Same CPU mode setup as
; common/reset.asm, then the run-time start-up _c_int00 of rts55x.lib (initialises the
; variables from .cinit, sets the stacks, calls main()).
	.mmregs
	.cpl_on
	.arms_on
	.c54cm_off

	.global _zen_reset
	.global _fw_reset
	.global _c_int00

	.sect ".text:_zen_reset"
_zen_reset:
_fw_reset:
	bit(ST1, #11) = #1
	@IMR0_L = #0 || mmap()
	@IMR1_L = #0 || mmap()
	@ST1_L = @ST1_L & #63775 || mmap()
	@ST1_L = @ST1_L | #16640 || mmap()
	@ST2_L = @ST2_L & #64000 || mmap()
	@ST2_L = @ST2_L | #32768 || mmap()
	bit(ST3, #5) = #0
	bit(ST3, #1) = #1
	@IVPD_L = #1 || mmap()          ; vectors at byte 0x100 (.vectors in doom.cmd)
	@IVPH_L = #1 || mmap()
	goto #_c_int00
