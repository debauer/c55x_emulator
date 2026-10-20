/* SPDX-License-Identifier: GPL-2.0-or-later
 * Parts of Doom that the port leaves out: joystick, ENDOOM screen, WAD checksum (network games) */
#include "zen_types.h"
#include "doomtype.h"
#include "i_endoom.h"
#include "i_joystick.h"
#include "sha1.h"
#include "w_checksum.h"

void I_InitJoystick(void) {}
void I_BindJoystickVariables(void) {}
void I_Endoom(byte *data) { (void) data; }
void W_Checksum(sha1_digest_t digest) { memset(digest, 0, sizeof(sha1_digest_t)); }
